//! Simulated project trees ([API §6.5]; [60 §4.2] row "File links"): per tree its canonical root, volume and
//! `VolumeCaps`, the files (bytes, file id, creation, write, change and added times, attributes) and the directories
//! (ids and times); the operations `write`, `mv`, `cp`, `rm`, `mkdir`, `attrs` and `deny`; and the reads the resolver
//! makes over them — `stat` under the directory's equivalence, enumeration, `locate_id` over the volume, the on-disk
//! spelling.
//!
//! Two trees on one volume share its file-id space and rename into each other; a rename to an absolute path outside
//! every tree (a quarantine outside the root, the Recycle Bin) keeps the file on its volume, where `locate_id` finds it
//! ([40 §4.3] step 5: "moved outside the root", "in the Recycle Bin").
//!
//! **Times** follow the file systems the model stands for ([F20 §5.12]; [80 §2.11.3]): creating, removing or renaming an
//! entry sets its parent directory's last-write and change times (POSIX `rename` updates both parents); a rename sets the
//! change time of the renamed entry itself — a file, or a directory, never the entries below a renamed directory — where
//! the volume's `ctime_on_rename` holds; a rename into another directory sets the entry's added time (macOS
//! `ADDEDTIME`); a rewrite sets the file's last-write and change times; an attribute change its change time.
//!
//! The simulated equivalence of an insensitive directory ([OS/project §4.5]) is the model's definition: a
//! case-insensitive directory equates names whose code points agree after `CaseFolding.txt`'s common (`C`) one-to-one
//! mappings; a normalization-insensitive one equates canonically equivalent names (NFD). Both are subsets of `fold_v1`
//! equality, as [F20 §3.3] requires of every directory.
//!
//! Each tree keeps three indexes beside its maps — the names directly inside every directory, the paths of every file
//! id and the path of every directory id — so a stat walks one directory per path segment and `locate_id` is one lookup
//! per tree. The maps are changed only by the operations of [`Fs`], which keep the indexes in step.

use crate::r4::fold::{cps, nfd_cps};
use crate::r4::path::Os;
use crate::r4::ucd::ucd;
use std::collections::{BTreeMap, BTreeSet};

/// `VolumeCaps.btime` ([OS/project §4.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btime {
    /// 0 absent.
    Absent,
    /// 1 tunneled-not-copied.
    TunneledNotCopied,
    /// 2 unforgeable.
    Unforgeable,
    /// 3 copied-by-clones.
    CopiedByClones,
}

/// `VolumeCaps.case_rule` ([OS/project §4.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseRule {
    /// 0 sensitive.
    Sensitive,
    /// 1 a per-directory flag.
    PerDirFlag,
    /// 2 the volume.
    Volume,
}

/// The capabilities of a volume ([OS/project §4.2]; [F11 §12.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeCaps {
    /// `id_kind`: 0 none (no trusted ids), 1 `ntfs128`, 2 `refs128`, 3 `linux_ino`, 4 `darwin_fileid`.
    pub id_kind: u8,
    /// `id_locate` is by-id (Windows, macOS) or a frontier (Linux); `false` for none.
    pub id_locate: bool,
    /// `btime`.
    pub btime: Btime,
    /// `ctime_on_rename`; `None` for unverified.
    pub ctime_on_rename: Option<bool>,
    /// `case_rule`.
    pub case_rule: CaseRule,
    /// `case_insensitive_default`.
    pub case_insensitive_default: bool,
    /// `norm_insensitive_always`.
    pub norm_insensitive_always: bool,
    /// `norm_follows_case`.
    pub norm_follows_case: bool,
    /// Cloud attributes are recognised (`cloud` ≠ none).
    pub cloud: bool,
    /// `clone_indicators`.
    pub clone_indicators: bool,
    /// The nominal timestamp resolution in ns ([OS/project §4.3] "nominal mtime").
    pub nominal_ns: u64,
    /// `mtime_granularity_ns`: the measured effective granularity; 0 = not measured.
    pub mtime_granularity_ns: u64,
    /// `dir_flush_doubtful`: a directory flush may be refused ([OS/project §6.2]).
    pub dir_flush_doubtful: bool,
}

impl VolumeCaps {
    /// Windows NTFS ([OS/project §4.3] row 1, the drafts of HOLE F20-btime-ntfs and F20-ctime-rename): the default of a
    /// simulated tree ([API §6.5]).
    pub const NTFS: VolumeCaps = VolumeCaps {
        id_kind: 1,
        id_locate: true,
        btime: Btime::TunneledNotCopied,
        ctime_on_rename: Some(true),
        case_rule: CaseRule::PerDirFlag,
        case_insensitive_default: true,
        norm_insensitive_always: false,
        norm_follows_case: false,
        cloud: true,
        clone_indicators: false,
        nominal_ns: 100,
        mtime_granularity_ns: 0,
        dir_flush_doubtful: false,
    };

    /// Linux ext4 ([OS/project §4.3]).
    pub const EXT4: VolumeCaps = VolumeCaps {
        id_kind: 3,
        id_locate: true,
        btime: Btime::Unforgeable,
        ctime_on_rename: Some(true),
        case_rule: CaseRule::PerDirFlag,
        case_insensitive_default: false,
        norm_insensitive_always: false,
        norm_follows_case: true,
        cloud: false,
        clone_indicators: false,
        nominal_ns: 1,
        mtime_granularity_ns: 0,
        dir_flush_doubtful: false,
    };

    /// macOS APFS, case-insensitive ([OS/project §4.3]).
    pub const APFS: VolumeCaps = VolumeCaps {
        id_kind: 4,
        id_locate: true,
        btime: Btime::CopiedByClones,
        ctime_on_rename: None,
        case_rule: CaseRule::Volume,
        case_insensitive_default: true,
        norm_insensitive_always: true,
        norm_follows_case: false,
        cloud: true,
        clone_indicators: true,
        nominal_ns: 1,
        mtime_granularity_ns: 0,
        dir_flush_doubtful: false,
    };

    /// Windows FAT32 or exFAT ([OS/project §4.3]): no trusted ids.
    pub const FAT: VolumeCaps = VolumeCaps {
        id_kind: 0,
        id_locate: false,
        btime: Btime::Absent,
        ctime_on_rename: None,
        case_rule: CaseRule::Volume,
        case_insensitive_default: true,
        norm_insensitive_always: false,
        norm_follows_case: false,
        cloud: false,
        clone_indicators: false,
        nominal_ns: 10_000_000,
        mtime_granularity_ns: 0,
        dir_flush_doubtful: false,
    };

    /// The granularity G of the volume's timestamps in ns ([F20 §1.2]: `max(10^gran, mtime_granularity_ns)`).
    pub fn granularity(&self) -> u64 {
        self.nominal_ns.max(self.mtime_granularity_ns).max(1)
    }
}

/// A file of a simulated tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimFile {
    /// Its file id on its volume.
    pub id: u64,
    /// Its bytes.
    pub bytes: Vec<u8>,
    /// Creation time, ns.
    pub btime_ns: i64,
    /// Last-write time, ns.
    pub mtime_ns: i64,
    /// Change time, ns: set by a write, a rename of this file (where the volume's `ctime_on_rename` holds), a copy and
    /// an attribute change; never by a rename of a directory above it.
    pub ctime_ns: i64,
    /// Added time, ns (macOS `ADDEDTIME`): set when the file is created or copied, and by a rename into another
    /// directory.
    pub added_ns: i64,
    /// Attribute names (`cloud-only`, `read-only`, `clone`, …).
    pub attrs: BTreeSet<String>,
    /// Reads fail with access denied.
    pub denied: bool,
}

/// A directory of a simulated tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimDir {
    /// Its id.
    pub id: u64,
    /// The per-directory case flag where the volume has one ([OS/project §4.5]); `None` takes the volume default.
    pub case_sensitive: Option<bool>,
    /// Creation time, ns.
    pub btime_ns: i64,
    /// Last-write time, ns: set when an entry directly inside it is created, removed or renamed.
    pub mtime_ns: i64,
    /// Change time, ns: set with the last-write time, and by a rename of the directory itself where the volume's
    /// `ctime_on_rename` holds.
    pub ctime_ns: i64,
}

/// One simulated tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    /// The canonical top level.
    pub root: String,
    /// The volume name.
    pub volume: String,
    /// The volume's capabilities.
    pub caps: VolumeCaps,
    /// The OS the tree is read on.
    pub os: Os,
    /// Root-relative path → file. Changed only by [`Fs`]'s operations.
    pub files: BTreeMap<String, SimFile>,
    /// Root-relative directory path → directory; the root itself is the empty path. Changed only by [`Fs`]'s
    /// operations.
    pub dirs: BTreeMap<String, SimDir>,
    /// The tree is a cloud sync root (the `TREES` cloud-root flag).
    pub cloud_root: bool,
    /// Directory path → the names directly inside it (files and directories).
    children: BTreeMap<String, BTreeSet<String>>,
    /// File id → the paths that name it (more than one for a hard-linked file).
    names: BTreeMap<u64, BTreeSet<String>>,
    /// Directory id → its path.
    dir_paths: BTreeMap<u64, String>,
}

/// A file identity ([F11 §12.1]; [F20 §5.3]): kind, volume and id; kind 0 equals nothing.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId {
    /// The kind (`VolumeCaps.id_kind`).
    pub kind: u8,
    /// The volume.
    pub volume: String,
    /// The object's id.
    pub id: u64,
}

impl FileId {
    /// Identity by the whole id ([F20 §5.3]): equal kind (≠ 0), volume and id.
    // spec: [F20 §5.3]; [F11 §12.1]
    pub fn same(&self, o: &FileId) -> bool {
        self.kind != 0 && self == o
    }
}

/// What a stat reads ([OS/project §5.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    /// The path as the directories spell it on disk.
    pub disk_path: String,
    /// The identity (the parent directory's id beside it).
    pub id: FileId,
    /// The parent directory's id (0 for the root).
    pub parent: u64,
    /// The size.
    pub size: u64,
    /// Last-write time, ns.
    pub mtime_ns: i64,
    /// Creation time, ns.
    pub btime_ns: i64,
    /// Change time, ns.
    pub ctime_ns: i64,
    /// Added time, ns.
    pub added_ns: i64,
    /// Attribute names.
    pub attrs: BTreeSet<String>,
    /// The number of names of the file in its tree (1 unless hard-linked).
    pub nlink: u32,
}

/// The outcome of a stat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatOut {
    /// The path is present.
    Present(Box<Stat>),
    /// No such entry.
    Absent,
    /// Access denied: presence unknown ([F20 §4.8]).
    Denied,
}

/// Where `locate_id` found an object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// In a tree: its root and the root-relative path.
    Tree(String, String),
    /// On the volume outside every tree: its absolute path.
    Elsewhere(String),
}

/// An error of a tree operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpError {
    /// The source does not exist.
    NotFound(String),
    /// The destination exists, or a path component is a file.
    Exists(String),
    /// The rename crosses volumes.
    CrossVolume,
    /// The tree is unknown.
    NoTree(String),
}

/// One operation of `EnvTree` ([API §6.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeOp {
    /// `write`: creates a file (next id, times now) or rewrites one (id and creation time kept).
    Write {
        /// The path.
        path: String,
        /// The bytes.
        bytes: Vec<u8>,
        /// An explicit creation time (tunneling, a preserved time).
        btime_ns: Option<i64>,
    },
    /// `mv`: a rename; the id, creation time and bytes move with the file, parents are created, `to` must not exist.
    /// `to` may be an absolute path: into another tree on the volume, or outside every tree.
    Mv {
        /// The source.
        from: String,
        /// The destination.
        to: String,
    },
    /// `cp`: a new file with a new id and the same bytes; creation time now, or the source's with `keep_btime`.
    Cp {
        /// The source.
        from: String,
        /// The destination.
        to: String,
        /// Keep the source's creation time.
        keep_btime: bool,
    },
    /// `rm`: a file, or a directory with everything below it.
    Rm {
        /// The path.
        path: String,
    },
    /// `mkdir`.
    Mkdir {
        /// The path.
        path: String,
        /// The per-directory case flag.
        case_sensitive: Option<bool>,
    },
    /// `attrs`.
    Attrs {
        /// The path.
        path: String,
        /// Names set.
        set: Vec<String>,
        /// Names cleared.
        clear: Vec<String>,
    },
    /// `deny`.
    Deny {
        /// The path.
        path: String,
        /// On or off.
        on: bool,
    },
}

/// The simulated file systems: trees, volumes and what lies on a volume outside every tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fs {
    /// Trees by canonical root.
    pub trees: BTreeMap<String, Tree>,
    /// The next object id per volume.
    pub next_id: BTreeMap<String, u64>,
    /// Files on a volume outside every tree: (volume, absolute path) → file.
    pub elsewhere: BTreeMap<(String, String), SimFile>,
    /// The home directory the Linux and macOS trash locations are under ([F20 §4.6]); `None` for none.
    pub home: Option<String>,
}

/// The parent of a root-relative path ("" for a path at the root).
pub fn dirname(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

/// The last component of a path.
pub fn basename(p: &str) -> &str {
    p.rfind('/').map_or(p, |i| &p[i + 1..])
}

/// The keys of a map that lie strictly below a directory (every key for the root), in key order: one range of the map,
/// since the keys below `d` are exactly those in [`d/`, `d0`) (`0` is the byte after `/`).
fn below<V>(m: &BTreeMap<String, V>, dir: &str) -> Vec<String> {
    if dir.is_empty() {
        return m.keys().filter(|k| !k.is_empty()).cloned().collect();
    }
    m.range(format!("{dir}/")..format!("{dir}0"))
        .map(|(k, _)| k.clone())
        .collect()
}

impl Tree {
    fn new(root: &str, volume: &str, caps: VolumeCaps, os: Os, root_id: u64) -> Tree {
        let mut t = Tree {
            root: root.to_string(),
            volume: volume.to_string(),
            caps,
            os,
            files: BTreeMap::new(),
            dirs: BTreeMap::new(),
            cloud_root: false,
            children: BTreeMap::new(),
            names: BTreeMap::new(),
            dir_paths: BTreeMap::new(),
        };
        t.add_dir(
            String::new(),
            SimDir {
                id: root_id,
                case_sensitive: None,
                btime_ns: 0,
                mtime_ns: 0,
                ctime_ns: 0,
            },
        );
        t
    }

    fn link(&mut self, path: &str) {
        self.children
            .entry(dirname(path).to_string())
            .or_default()
            .insert(basename(path).to_string());
    }

    fn unlink(&mut self, path: &str) {
        if let Some(c) = self.children.get_mut(dirname(path)) {
            c.remove(basename(path));
        }
    }

    fn add_file(&mut self, path: String, f: SimFile) {
        self.link(&path);
        self.names.entry(f.id).or_default().insert(path.clone());
        self.files.insert(path, f);
    }

    fn take_file(&mut self, path: &str) -> Option<SimFile> {
        let f = self.files.remove(path)?;
        self.unlink(path);
        if let Some(s) = self.names.get_mut(&f.id) {
            s.remove(path);
            if s.is_empty() {
                self.names.remove(&f.id);
            }
        }
        Some(f)
    }

    fn add_dir(&mut self, path: String, d: SimDir) {
        if !path.is_empty() {
            self.link(&path);
        }
        self.dir_paths.insert(d.id, path.clone());
        self.children.entry(path.clone()).or_default();
        self.dirs.insert(path, d);
    }

    fn take_dir(&mut self, path: &str) -> Option<SimDir> {
        let d = self.dirs.remove(path)?;
        if !path.is_empty() {
            self.unlink(path);
        }
        self.dir_paths.remove(&d.id);
        self.children.remove(path);
        Some(d)
    }

    /// Sets a directory's last-write and change times: an entry directly inside it was created, removed or renamed.
    fn touch(&mut self, dir: &str, now_ns: i64) {
        if let Some(d) = self.dirs.get_mut(dir) {
            d.mtime_ns = now_ns;
            d.ctime_ns = now_ns;
        }
    }
}

impl Fs {
    fn alloc(&mut self, volume: &str) -> u64 {
        let n = self.next_id.entry(volume.to_string()).or_insert(1);
        let id = *n;
        *n += 1;
        id
    }

    /// Creates a tree on first use ([API §6.5] "created on first use with `volume` and `caps`"); its root directory's
    /// times are 0 (it existed before every operation).
    pub fn ensure_tree(&mut self, root: &str, volume: &str, caps: VolumeCaps, os: Os) -> &mut Tree {
        if !self.trees.contains_key(root) {
            let id = self.alloc(volume);
            self.trees
                .insert(root.to_string(), Tree::new(root, volume, caps, os, id));
        }
        self.trees.get_mut(root).expect("the tree exists")
    }

    /// Creates the missing directories of `path` at `now_ns`, touching the parent of each; a component that is a file
    /// refuses.
    fn mkdirs(&mut self, root: &str, path: &str, now_ns: i64) -> Result<(), OpError> {
        let volume = self.trees[root].volume.clone();
        let mut acc = String::new();
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(seg);
            if self.trees[root].files.contains_key(&acc) {
                return Err(OpError::Exists(acc));
            }
            if !self.trees[root].dirs.contains_key(&acc) {
                let id = self.alloc(&volume);
                let t = self.trees.get_mut(root).expect("the tree exists");
                t.add_dir(
                    acc.clone(),
                    SimDir {
                        id,
                        case_sensitive: None,
                        btime_ns: now_ns,
                        mtime_ns: now_ns,
                        ctime_ns: now_ns,
                    },
                );
                t.touch(dirname(&acc), now_ns);
            }
        }
        Ok(())
    }

    /// The tree whose root is the longest prefix of an absolute path on a volume, with the root-relative rest.
    fn tree_of_abs(&self, volume: &str, abs: &str) -> Option<(String, String)> {
        self.trees
            .values()
            .filter(|t| {
                t.volume == volume && (abs == t.root || abs.starts_with(&format!("{}/", t.root)))
            })
            .max_by_key(|t| t.root.len())
            .map(|t| {
                let rest = abs[t.root.len()..].trim_start_matches('/').to_string();
                (t.root.clone(), rest)
            })
    }

    /// Applies one operation to a tree at the wall time `now_ns` ([API §6.5]).
    // spec: [API §6.5]
    pub fn apply(&mut self, root: &str, op: &TreeOp, now_ns: i64) -> Result<(), OpError> {
        if !self.trees.contains_key(root) {
            return Err(OpError::NoTree(root.to_string()));
        }
        match op {
            TreeOp::Write {
                path,
                bytes,
                btime_ns,
            } => {
                if self.trees[root].dirs.contains_key(path) {
                    return Err(OpError::Exists(path.clone()));
                }
                self.mkdirs(root, dirname(path), now_ns)?;
                let volume = self.trees[root].volume.clone();
                let exists = self.trees[root].files.contains_key(path);
                if exists {
                    let f = self
                        .trees
                        .get_mut(root)
                        .and_then(|t| t.files.get_mut(path))
                        .expect("the file exists");
                    f.bytes = bytes.clone();
                    f.mtime_ns = now_ns;
                    f.ctime_ns = now_ns;
                    if let Some(b) = btime_ns {
                        f.btime_ns = *b;
                    }
                } else {
                    let id = self.alloc(&volume);
                    let t = self.trees.get_mut(root).expect("the tree exists");
                    t.add_file(
                        path.clone(),
                        SimFile {
                            id,
                            bytes: bytes.clone(),
                            btime_ns: btime_ns.unwrap_or(now_ns),
                            mtime_ns: now_ns,
                            ctime_ns: now_ns,
                            added_ns: now_ns,
                            attrs: BTreeSet::new(),
                            denied: false,
                        },
                    );
                    t.touch(dirname(path), now_ns);
                }
                Ok(())
            }
            TreeOp::Mv { from, to } => self.mv(root, from, to, now_ns),
            TreeOp::Cp {
                from,
                to,
                keep_btime,
            } => {
                let src = self.trees[root]
                    .files
                    .get(from)
                    .cloned()
                    .ok_or_else(|| OpError::NotFound(from.clone()))?;
                if self.trees[root].files.contains_key(to) || self.trees[root].dirs.contains_key(to)
                {
                    return Err(OpError::Exists(to.clone()));
                }
                self.mkdirs(root, dirname(to), now_ns)?;
                let volume = self.trees[root].volume.clone();
                let id = self.alloc(&volume);
                let t = self.trees.get_mut(root).expect("the tree exists");
                t.add_file(
                    to.clone(),
                    SimFile {
                        id,
                        bytes: src.bytes,
                        btime_ns: if *keep_btime { src.btime_ns } else { now_ns },
                        mtime_ns: src.mtime_ns,
                        ctime_ns: now_ns,
                        added_ns: now_ns,
                        attrs: src.attrs,
                        denied: false,
                    },
                );
                t.touch(dirname(to), now_ns);
                Ok(())
            }
            TreeOp::Rm { path } => {
                let t = self.trees.get_mut(root).expect("the tree exists");
                if t.take_file(path).is_some() {
                    t.touch(dirname(path), now_ns);
                    return Ok(());
                }
                if path.is_empty() || !t.dirs.contains_key(path) {
                    return Err(OpError::NotFound(path.clone()));
                }
                for k in below(&t.files, path) {
                    t.take_file(&k);
                }
                let mut ds = below(&t.dirs, path);
                ds.reverse();
                for k in ds {
                    t.take_dir(&k);
                }
                t.take_dir(path);
                t.touch(dirname(path), now_ns);
                Ok(())
            }
            TreeOp::Mkdir {
                path,
                case_sensitive,
            } => {
                self.mkdirs(root, path, now_ns)?;
                if let Some(d) = self.trees.get_mut(root).and_then(|t| t.dirs.get_mut(path)) {
                    d.case_sensitive = *case_sensitive;
                }
                Ok(())
            }
            TreeOp::Attrs { path, set, clear } => {
                let f = self
                    .trees
                    .get_mut(root)
                    .and_then(|t| t.files.get_mut(path))
                    .ok_or_else(|| OpError::NotFound(path.clone()))?;
                for a in set {
                    f.attrs.insert(a.clone());
                }
                for a in clear {
                    f.attrs.remove(a);
                }
                f.ctime_ns = now_ns;
                Ok(())
            }
            TreeOp::Deny { path, on } => {
                let f = self
                    .trees
                    .get_mut(root)
                    .and_then(|t| t.files.get_mut(path))
                    .ok_or_else(|| OpError::NotFound(path.clone()))?;
                f.denied = *on;
                Ok(())
            }
        }
    }

    /// A rename of a file or a directory subtree, ids kept; into another tree of the volume or outside every tree when
    /// `to` is absolute. The renamed entry's change time is set where the volume's `ctime_on_rename` holds, its added
    /// time when its directory changes, and both parents are touched; entries below a renamed directory keep their
    /// times.
    fn mv(&mut self, root: &str, from: &str, to: &str, now_ns: i64) -> Result<(), OpError> {
        let volume = self.trees[root].volume.clone();
        let ctime_on_rename = self.trees[root].caps.ctime_on_rename == Some(true);
        let is_abs = |x: &str| x.starts_with('/') || (x.len() >= 3 && x.as_bytes()[1] == b':');
        if is_abs(from) {
            // From another tree of the volume, or back from outside every tree.
            if let Some((r2, rel)) = self.tree_of_abs(&volume, from) {
                let to_abs = if is_abs(to) {
                    to.to_string()
                } else {
                    format!("{root}/{to}")
                };
                return self.mv(&r2, &rel, &to_abs, now_ns);
            }
            let key = (volume.clone(), from.to_string());
            if !self.elsewhere.contains_key(&key) {
                return Err(OpError::NotFound(from.to_string()));
            }
            let (dr, rel) = if is_abs(to) {
                match self.tree_of_abs(&volume, to) {
                    Some(x) => x,
                    None => {
                        let mut f = self.elsewhere.remove(&key).expect("present");
                        if ctime_on_rename {
                            f.ctime_ns = now_ns;
                        }
                        f.added_ns = now_ns;
                        self.elsewhere.insert((volume, to.to_string()), f);
                        return Ok(());
                    }
                }
            } else {
                (root.to_string(), to.to_string())
            };
            if self.trees[&dr].files.contains_key(&rel) || self.trees[&dr].dirs.contains_key(&rel) {
                return Err(OpError::Exists(to.to_string()));
            }
            self.mkdirs(&dr, dirname(&rel), now_ns)?;
            let mut f = self.elsewhere.remove(&key).expect("present");
            if ctime_on_rename {
                f.ctime_ns = now_ns;
            }
            f.added_ns = now_ns;
            let t = self.trees.get_mut(&dr).expect("the tree exists");
            t.add_file(rel.clone(), f);
            t.touch(dirname(&rel), now_ns);
            return Ok(());
        }
        let absolute = is_abs(to);
        let (dst_root, dst_rel) = if absolute {
            match self.tree_of_abs(&volume, to) {
                Some(x) => (Some(x.0), x.1),
                None => (None, to.to_string()),
            }
        } else {
            (Some(root.to_string()), to.to_string())
        };
        let t = &self.trees[root];
        let is_file = t.files.contains_key(from);
        let is_dir = !from.is_empty() && t.dirs.contains_key(from);
        if !is_file && !is_dir {
            return Err(OpError::NotFound(from.to_string()));
        }
        if let Some(dr) = &dst_root {
            let d = &self.trees[dr];
            if d.files.contains_key(&dst_rel)
                || (!dst_rel.is_empty() && d.dirs.contains_key(&dst_rel))
                || (dr == root && is_dir && dst_rel.starts_with(&format!("{from}/")))
            {
                return Err(OpError::Exists(to.to_string()));
            }
        } else if self
            .elsewhere
            .contains_key(&(volume.clone(), dst_rel.clone()))
        {
            return Err(OpError::Exists(to.to_string()));
        }
        if let Some(dr) = &dst_root {
            self.mkdirs(dr, dirname(&dst_rel), now_ns)?;
        }
        let moves_dir = dst_root.as_deref() != Some(root) || dirname(from) != dirname(&dst_rel);
        // Take the moving entries out; only the renamed entry itself gets the new change and added times.
        let src = self.trees.get_mut(root).expect("the tree exists");
        let mut files: Vec<(String, SimFile)> = Vec::new();
        let mut dirs: Vec<(String, SimDir)> = Vec::new();
        if is_file {
            let mut f = src.take_file(from).expect("present");
            if ctime_on_rename {
                f.ctime_ns = now_ns;
            }
            if moves_dir {
                f.added_ns = now_ns;
            }
            files.push((String::new(), f));
        } else {
            for k in below(&src.files, from) {
                let f = src.take_file(&k).expect("present");
                files.push((k[from.len()..].to_string(), f));
            }
            let mut ds = below(&src.dirs, from);
            ds.reverse();
            for k in ds {
                let d = src.take_dir(&k).expect("present");
                dirs.push((k[from.len()..].to_string(), d));
            }
            let mut d = src.take_dir(from).expect("present");
            if ctime_on_rename {
                d.ctime_ns = now_ns;
            }
            dirs.push((String::new(), d));
            dirs.reverse();
        }
        src.touch(dirname(from), now_ns);
        match dst_root {
            Some(dr) => {
                let d = self.trees.get_mut(&dr).expect("the tree exists");
                for (rest, dir) in dirs {
                    d.add_dir(format!("{dst_rel}{rest}"), dir);
                }
                for (rest, f) in files {
                    d.add_file(format!("{dst_rel}{rest}"), f);
                }
                d.touch(dirname(&dst_rel), now_ns);
            }
            None => {
                for (rest, f) in files {
                    self.elsewhere
                        .insert((volume.clone(), format!("{dst_rel}{rest}")), f);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
impl Fs {
    /// A second name for an existing file of a tree (a hard link: one id, two paths), which [API §6.5]'s operations do
    /// not make; the tests of [F20 §5.3]'s hard-link cap build one with it.
    pub(crate) fn hard_link(&mut self, root: &str, from: &str, to: &str) {
        let f = self.trees[root]
            .files
            .get(from)
            .expect("the file exists")
            .clone();
        self.mkdirs(root, dirname(to), f.ctime_ns)
            .expect("the parents can be made");
        let t = self.trees.get_mut(root).expect("the tree exists");
        t.add_file(to.to_string(), f);
    }
}

impl Fs {
    /// Writes a committed tree into a simulated tree ([API §6.6]: "the harness writes a repository that realises the
    /// same abstract history"; here the reverse, a checkout): every path of τ(c) with the bytes `content` gives its blob.
    pub fn checkout(
        &mut self,
        root: &str,
        tau: &BTreeMap<String, String>,
        content: impl Fn(&str) -> Vec<u8>,
        now_ns: i64,
    ) -> Result<(), OpError> {
        for (p, blob) in tau {
            self.apply(
                root,
                &TreeOp::Write {
                    path: p.clone(),
                    bytes: content(blob),
                    btime_ns: None,
                },
                now_ns,
            )?;
        }
        Ok(())
    }
}

/// Whether two names are equal under a simulated directory's equivalence (module documentation).
pub fn equiv(a: &str, b: &str, case_insensitive: bool, norm_insensitive: bool) -> bool {
    if a == b {
        return true;
    }
    let u = ucd();
    let key = |x: &str| -> Vec<u32> {
        let mut v = cps(x);
        if norm_insensitive {
            v = nfd_cps(u, &v);
        }
        if case_insensitive {
            v = v
                .into_iter()
                .map(|c| u.common_folding(c).unwrap_or(c))
                .collect();
            if norm_insensitive {
                v = nfd_cps(u, &v);
            }
        }
        v
    };
    (case_insensitive || norm_insensitive) && key(a) == key(b)
}

impl Tree {
    /// The creation time a `FILEOBS` row records for a file of this tree whose stat read `btime_ns` ([F20 §5.2] stat
    /// quadruple; [F11 §12.5] `creation`): absent on a volume whose `VolumeCaps.btime` is `Absent`, which has no
    /// creation time to read. Every writer of the row — a settle, a file verb — records it by this one rule.
    pub fn recorded_creation(&self, btime_ns: i64) -> Option<i64> {
        (self.caps.btime != Btime::Absent).then_some(btime_ns)
    }

    /// The equivalence a directory observes ([OS/project §4.5]): (case-insensitive, normalization-insensitive).
    pub fn dir_equivalence(&self, dir: &str) -> (bool, bool) {
        let c = &self.caps;
        let ci = match c.case_rule {
            CaseRule::Sensitive => false,
            CaseRule::Volume => c.case_insensitive_default,
            CaseRule::PerDirFlag => self
                .dirs
                .get(dir)
                .and_then(|d| d.case_sensitive)
                .map_or(c.case_insensitive_default, |s| !s),
        };
        let ni = c.norm_insensitive_always || (c.norm_follows_case && ci);
        (ci, ni)
    }

    /// The on-disk spelling of a root-relative path when every component exists under its directory's equivalence:
    /// each component replaced by the entry the directory holds (the exact name when it exists).
    pub fn disk_spelling(&self, path: &str) -> Option<String> {
        let mut acc = String::new();
        let segs: Vec<&str> = path.split('/').collect();
        for (i, seg) in segs.iter().enumerate() {
            let last = i + 1 == segs.len();
            let join = |name: &str| {
                if acc.is_empty() {
                    name.to_string()
                } else {
                    format!("{acc}/{name}")
                }
            };
            let exact = join(seg);
            let found =
                if (last && self.files.contains_key(&exact)) || self.dirs.contains_key(&exact) {
                    Some(exact)
                } else {
                    let (ci, ni) = self.dir_equivalence(&acc);
                    self.children.get(&acc).and_then(|names| {
                        names
                            .iter()
                            .find(|n| {
                                let p = join(n);
                                equiv(n, seg, ci, ni)
                                    && if last {
                                        self.files.contains_key(&p) || self.dirs.contains_key(&p)
                                    } else {
                                        self.dirs.contains_key(&p)
                                    }
                            })
                            .map(|n| join(n))
                    })
                };
            acc = found?;
        }
        Some(acc)
    }

    /// The names directly inside a directory (files and directories), in byte order.
    pub fn entries(&self, dir: &str) -> Vec<String> {
        self.children
            .get(dir)
            .map(|c| c.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The regular files directly inside a directory, as root-relative paths in byte order ([F20 §4.1]).
    pub fn files_in(&self, dir: &str) -> Vec<String> {
        let join = |n: &String| {
            if dir.is_empty() {
                n.clone()
            } else {
                format!("{dir}/{n}")
            }
        };
        self.children
            .get(dir)
            .map(|c| {
                c.iter()
                    .map(join)
                    .filter(|p| self.files.contains_key(p))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The directories directly inside a directory, as root-relative paths in byte order.
    pub fn dirs_in(&self, dir: &str) -> Vec<String> {
        let join = |n: &String| {
            if dir.is_empty() {
                n.clone()
            } else {
                format!("{dir}/{n}")
            }
        };
        self.children
            .get(dir)
            .map(|c| {
                c.iter()
                    .map(join)
                    .filter(|p| self.dirs.contains_key(p))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The id of a directory, if it exists.
    pub fn dir_id(&self, dir: &str) -> Option<u64> {
        self.dirs.get(dir).map(|d| d.id)
    }

    /// `stat` of a root-relative path under the directories' equivalence ([OS/project §5.1], §5.3).
    // spec: [OS/project §5.1]; [OS/project §5.3]
    pub fn stat(&self, path: &str) -> StatOut {
        let Some(disk) = self.disk_spelling(path) else {
            return StatOut::Absent;
        };
        let Some(f) = self.files.get(&disk) else {
            return StatOut::Absent;
        };
        if f.denied {
            return StatOut::Denied;
        }
        let nlink = self.names.get(&f.id).map_or(1, |s| s.len() as u32);
        StatOut::Present(Box::new(Stat {
            parent: self.dir_id(dirname(&disk)).unwrap_or(0),
            disk_path: disk,
            id: FileId {
                kind: self.caps.id_kind,
                volume: self.volume.clone(),
                id: f.id,
            },
            size: f.bytes.len() as u64,
            mtime_ns: f.mtime_ns,
            btime_ns: f.btime_ns,
            ctime_ns: f.ctime_ns,
            added_ns: f.added_ns,
            attrs: f.attrs.clone(),
            nlink,
        }))
    }

    /// The bytes of a file at its exact on-disk path, or why they are unavailable (a detail code of [F18 §4.6]:
    /// 54 cloud-only, 59 unreadable).
    pub fn read(&self, disk_path: &str) -> Result<&[u8], u8> {
        let f = self.files.get(disk_path).ok_or(59u8)?;
        if f.denied {
            return Err(59);
        }
        if self.caps.cloud && f.attrs.contains("cloud-only") {
            return Err(54);
        }
        Ok(&f.bytes)
    }

    /// Whether an entry is cloud-only ([F20 §4.5]).
    pub fn cloud_only(&self, disk_path: &str) -> bool {
        self.caps.cloud
            && self
                .files
                .get(disk_path)
                .is_some_and(|f| f.attrs.contains("cloud-only"))
    }
}

impl Fs {
    /// `locate_id`: the object with this identity on its volume, in a tree or elsewhere ([OS/project §5.4]); `None`
    /// when the volume has no trusted ids or none holds it. A hard-linked file is located at its first name in path
    /// order.
    // spec: [OS/project §5.4]
    pub fn locate_id(&self, id: &FileId) -> Option<Location> {
        if id.kind == 0 {
            return None;
        }
        for t in self
            .trees
            .values()
            .filter(|t| t.volume == id.volume && t.caps.id_kind == id.kind)
        {
            if let Some(p) = t.names.get(&id.id).and_then(|s| s.iter().next()) {
                return Some(Location::Tree(t.root.clone(), p.clone()));
            }
        }
        self.elsewhere
            .iter()
            .find(|((v, _), f)| *v == id.volume && f.id == id.id)
            .map(|((_, p), _)| Location::Elsewhere(p.clone()))
    }

    /// `locate_id` for a directory id ([40 §4.3] E3d).
    pub fn locate_dir(&self, volume: &str, kind: u8, id: u64) -> Option<Location> {
        if kind == 0 {
            return None;
        }
        self.trees
            .values()
            .filter(|t| t.volume == volume && t.caps.id_kind == kind)
            .find_map(|t| {
                t.dir_paths
                    .get(&id)
                    .map(|p| Location::Tree(t.root.clone(), p.clone()))
            })
    }

    /// Whether an absolute location lies in a trash location of the OS ([F20 §4.6]).
    // spec: [F20 §4.6]
    pub fn in_trash(&self, os: Os, abs: &str) -> bool {
        match os {
            Os::Windows => {
                // `$Recycle.Bin` at the root of any volume, compared with `eqi`.
                let rest = if abs.as_bytes().get(1) == Some(&b':') {
                    abs.get(3..).unwrap_or("")
                } else {
                    abs.trim_start_matches('/')
                };
                rest.split('/')
                    .next()
                    .is_some_and(|c| c.eq_ignore_ascii_case("$Recycle.Bin"))
            }
            Os::Linux => {
                let home_trash = self
                    .home
                    .as_ref()
                    .is_some_and(|h| abs.starts_with(&format!("{h}/.local/share/Trash/")));
                home_trash
                    || abs
                        .split('/')
                        .any(|c| c == ".Trash" || c.starts_with(".Trash-"))
            }
            Os::Macos => {
                let home_trash = self
                    .home
                    .as_ref()
                    .is_some_and(|h| abs.starts_with(&format!("{h}/.Trash/")));
                home_trash || abs.split('/').any(|c| c == ".Trashes")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs() -> Fs {
        let mut f = Fs::default();
        f.ensure_tree("C:/work/repo", "C", VolumeCaps::NTFS, Os::Windows);
        f
    }

    fn w(path: &str, b: &str) -> TreeOp {
        TreeOp::Write {
            path: path.into(),
            bytes: b.as_bytes().to_vec(),
            btime_ns: None,
        }
    }

    #[test]
    fn writes_renames_and_copies_keep_or_mint_ids() {
        let mut f = fs();
        let r = "C:/work/repo";
        f.apply(r, &w("src/a.rs", "x"), 10).unwrap();
        let id = f.trees[r].files["src/a.rs"].id;
        f.apply(r, &w("src/a.rs", "y"), 20).unwrap();
        assert_eq!(
            f.trees[r].files["src/a.rs"].id, id,
            "a rewrite keeps the id"
        );
        assert_eq!(f.trees[r].files["src/a.rs"].btime_ns, 10);
        f.apply(
            r,
            &TreeOp::Mv {
                from: "src".into(),
                to: "lib".into(),
            },
            30,
        )
        .unwrap();
        assert_eq!(f.trees[r].files["lib/a.rs"].id, id);
        assert_eq!(
            f.trees[r].files["lib/a.rs"].ctime_ns, 20,
            "a directory rename leaves the change time of the files below it"
        );
        assert_eq!(
            f.trees[r].dirs["lib"].ctime_ns, 30,
            "NTFS sets the renamed directory's ChangeTime"
        );
        f.apply(
            r,
            &TreeOp::Mv {
                from: "lib/a.rs".into(),
                to: "lib/c.rs".into(),
            },
            35,
        )
        .unwrap();
        assert_eq!(
            f.trees[r].files["lib/c.rs"].ctime_ns, 35,
            "NTFS sets a renamed file's ChangeTime"
        );
        assert_eq!(
            f.trees[r].files["lib/c.rs"].added_ns, 10,
            "a rename inside one directory keeps the added time"
        );
        f.apply(
            r,
            &TreeOp::Mv {
                from: "lib/c.rs".into(),
                to: "lib/a.rs".into(),
            },
            36,
        )
        .unwrap();
        f.apply(
            r,
            &TreeOp::Cp {
                from: "lib/a.rs".into(),
                to: "lib/b.rs".into(),
                keep_btime: true,
            },
            40,
        )
        .unwrap();
        assert_ne!(f.trees[r].files["lib/b.rs"].id, id);
        assert_eq!(f.trees[r].files["lib/b.rs"].btime_ns, 10);
        assert_eq!(
            f.trees[r].dirs["lib"].mtime_ns, 40,
            "a new entry touches its directory"
        );
        f.apply(
            r,
            &TreeOp::Mv {
                from: "lib/a.rs".into(),
                to: "C:/$Recycle.Bin/a.rs".into(),
            },
            50,
        )
        .unwrap();
        let loc = f
            .locate_id(&FileId {
                kind: 1,
                volume: "C".into(),
                id,
            })
            .unwrap();
        assert_eq!(loc, Location::Elsewhere("C:/$Recycle.Bin/a.rs".into()));
        assert!(f.in_trash(Os::Windows, "C:/$Recycle.Bin/a.rs"));
        assert!(!f.in_trash(Os::Windows, "C:/work/repo/a.rs"));
        // Back from the Recycle Bin into another directory.
        f.apply(
            r,
            &TreeOp::Mv {
                from: "C:/$Recycle.Bin/a.rs".into(),
                to: "restored/a.rs".into(),
            },
            60,
        )
        .unwrap();
        assert_eq!(f.trees[r].files["restored/a.rs"].id, id);
        assert_eq!(f.trees[r].files["restored/a.rs"].added_ns, 60);
    }

    #[test]
    fn the_indexes_follow_every_operation() {
        let mut f = fs();
        let r = "C:/work/repo";
        f.apply(r, &w("a/b/x.rs", "x"), 1).unwrap();
        f.apply(r, &w("a/y.rs", "y"), 1).unwrap();
        let t = &f.trees[r];
        assert_eq!(t.entries(""), ["a"]);
        assert_eq!(t.entries("a"), ["b", "y.rs"]);
        assert_eq!(t.files_in("a"), ["a/y.rs"]);
        assert_eq!(t.dirs_in("a"), ["a/b"]);
        let bid = t.dir_id("a/b").unwrap();
        f.apply(
            r,
            &TreeOp::Mv {
                from: "a".into(),
                to: "z/a2".into(),
            },
            2,
        )
        .unwrap();
        let t = &f.trees[r];
        assert_eq!(t.entries(""), ["z"]);
        assert_eq!(t.files_in("z/a2/b"), ["z/a2/b/x.rs"]);
        assert_eq!(
            f.locate_dir("C", 1, bid),
            Some(Location::Tree(r.into(), "z/a2/b".into()))
        );
        f.apply(r, &TreeOp::Rm { path: "z".into() }, 3).unwrap();
        let t = &f.trees[r];
        assert!(t.files.is_empty() && t.entries("").is_empty());
        assert_eq!(t.dirs.len(), 1, "only the root is left");
        assert_eq!(f.locate_dir("C", 1, bid), None);
        // A file where a directory is needed, and a directory renamed into itself, refuse.
        f.apply(r, &w("f", "x"), 4).unwrap();
        assert_eq!(
            f.apply(r, &w("f/g", "y"), 5),
            Err(OpError::Exists("f".into()))
        );
        f.apply(r, &w("d/e", "y"), 5).unwrap();
        assert!(
            f.apply(
                r,
                &TreeOp::Mv {
                    from: "d".into(),
                    to: "d/sub".into()
                },
                6
            )
            .is_err()
        );
    }

    #[test]
    fn stat_follows_the_directory_equivalence() {
        let mut f = fs();
        let r = "C:/work/repo";
        f.apply(r, &w("Docs/Plan.md", "x"), 1).unwrap();
        let t = &f.trees[r];
        match t.stat("docs/plan.md") {
            StatOut::Present(s) => assert_eq!(s.disk_path, "Docs/Plan.md"),
            o => panic!("{o:?}"),
        }
        assert_eq!(t.stat("docs/plan2.md"), StatOut::Absent);
        // NTFS is normalization-sensitive.
        f.apply(r, &w("caf\u{e9}.md", "a"), 2).unwrap();
        assert_eq!(f.trees[r].stat("cafe\u{301}.md"), StatOut::Absent);
        let mut g = Fs::default();
        g.ensure_tree("/home/u/r", "sda", VolumeCaps::EXT4, Os::Linux);
        g.apply("/home/u/r", &w("A.md", "x"), 1).unwrap();
        assert_eq!(g.trees["/home/u/r"].stat("a.md"), StatOut::Absent);
        assert!(equiv("cafe\u{301}", "CAF\u{c9}", true, true));
        assert!(
            !equiv("stra\u{df}e", "STRASSE", true, false),
            "common folding does not merge ß and ss"
        );
    }
}
