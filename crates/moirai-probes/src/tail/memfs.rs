//! An in-memory `StoreFs + SealedMaps` for the unit tests of [`super::store`] and `m10`: directories and files by
//! absolute path, sealed flags, and a log of every open, read and map, so a test can count what an arm did. A hook
//! runs on every `open` (the tests advance a fake clock there). Only what the store module calls is implemented; every
//! other call fails with `Unsupported`.

use moirai_vfs::{
    Access, Advice, DirEntry, DurabilityFailure, EntryKind, EntryName, FileIdentity, FreeSpace,
    GroupMember, MapError, OpenHint, OsCode, RelPath, RootAccess, RootRole, SealedMap, SealedMaps,
    ShareRetry, StoreFs, StoreVolume, SwapOutcome, SwapRecovery, SyncKind, VfsCounters, VfsError,
    VfsErrorKind, VfsTypes,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What happened, in order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Log {
    /// Every `open`: the file's name.
    pub opens: Vec<String>,
    /// Every read: (name, offset, bytes asked).
    pub reads: Vec<(String, u64, usize)>,
    /// Every `map_sealed`: (name, length).
    pub maps: Vec<(String, u64)>,
}

#[derive(Default)]
struct State {
    dirs: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, Vec<u8>>,
    sealed: BTreeSet<PathBuf>,
    log: Log,
}

/// The in-memory store.
#[derive(Default)]
pub struct MemFs {
    state: RefCell<State>,
    on_open: Option<Box<dyn Fn()>>,
}

/// An open file.
#[derive(Clone, Debug)]
pub struct MemFile {
    path: PathBuf,
}

/// A mapping: a copy of the file's bytes.
#[derive(Debug)]
pub struct MemMap(Arc<Vec<u8>>);

impl SealedMap for MemMap {
    fn bytes(&self) -> &[u8] {
        &self.0
    }

    fn len(&self) -> u64 {
        self.0.len() as u64
    }
}

fn err(kind: VfsErrorKind, call: &'static str) -> VfsError {
    VfsError::new(kind, OsCode::NONE, call)
}

fn name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl MemFs {
    /// An empty store whose root directory `dir` exists.
    pub fn with_dir(dir: &Path) -> MemFs {
        let fs = MemFs::default();
        fs.state.borrow_mut().dirs.insert(dir.to_path_buf());
        fs
    }

    /// A store that calls `hook` on every open.
    pub fn with_hook(dir: &Path, hook: impl Fn() + 'static) -> MemFs {
        let mut fs = MemFs::with_dir(dir);
        fs.on_open = Some(Box::new(hook));
        fs
    }

    /// The log so far.
    pub fn log(&self) -> Log {
        self.state.borrow().log.clone()
    }

    /// Clears the log.
    pub fn clear_log(&self) {
        self.state.borrow_mut().log = Log::default();
    }

    /// The files and their lengths.
    pub fn files(&self) -> BTreeMap<PathBuf, usize> {
        self.state
            .borrow()
            .files
            .iter()
            .map(|(p, b)| (p.clone(), b.len()))
            .collect()
    }

    /// A file's bytes.
    pub fn bytes(&self, p: &Path) -> Option<Vec<u8>> {
        self.state.borrow().files.get(p).cloned()
    }

    /// Whether a file is sealed.
    pub fn is_sealed(&self, p: &Path) -> bool {
        self.state.borrow().sealed.contains(p)
    }

    /// Whether a directory exists.
    pub fn has_dir(&self, p: &Path) -> bool {
        self.state.borrow().dirs.contains(p)
    }
}

impl VfsTypes for MemFs {
    type Root = PathBuf;
    type File = MemFile;
}

impl StoreFs for MemFs {
    fn open_root(
        &self,
        dir: &Path,
        _role: RootRole,
        _access: RootAccess,
    ) -> Result<PathBuf, VfsError> {
        if self.state.borrow().dirs.contains(dir) {
            Ok(dir.to_path_buf())
        } else {
            Err(err(VfsErrorKind::NotFound, "open_root"))
        }
    }

    fn create_root(&self, dir: &Path, _role: RootRole) -> Result<PathBuf, VfsError> {
        let mut s = self.state.borrow_mut();
        if !dir.parent().is_some_and(|p| s.dirs.contains(p)) {
            return Err(err(VfsErrorKind::NotFound, "create_root"));
        }
        if !s.dirs.insert(dir.to_path_buf()) {
            return Err(err(VfsErrorKind::AlreadyExists, "create_root"));
        }
        Ok(dir.to_path_buf())
    }

    fn open(
        &self,
        root: &PathBuf,
        rel: RelPath<'_>,
        _access: Access,
        _hint: OpenHint,
    ) -> Result<MemFile, VfsError> {
        let path = root.join(rel.as_str());
        if !self.state.borrow().files.contains_key(&path) {
            return Err(err(VfsErrorKind::NotFound, "open"));
        }
        self.state.borrow_mut().log.opens.push(name(&path));
        if let Some(h) = &self.on_open {
            h();
        }
        Ok(MemFile { path })
    }

    fn create_new(&self, root: &PathBuf, rel: RelPath<'_>) -> Result<MemFile, VfsError> {
        let path = root.join(rel.as_str());
        let mut s = self.state.borrow_mut();
        if !s.dirs.contains(root) {
            return Err(err(VfsErrorKind::NotFound, "create_new"));
        }
        if s.files.contains_key(&path) {
            return Err(err(VfsErrorKind::AlreadyExists, "create_new"));
        }
        s.files.insert(path.clone(), Vec::new());
        Ok(MemFile { path })
    }

    fn create_dir(&self, root: &PathBuf, rel: RelPath<'_>) -> Result<(), VfsError> {
        self.state.borrow_mut().dirs.insert(root.join(rel.as_str()));
        Ok(())
    }

    fn remove_dir(&self, root: &PathBuf, rel: RelPath<'_>) -> Result<(), VfsError> {
        let dir = root.join(rel.as_str());
        let mut s = self.state.borrow_mut();
        if s.files.keys().any(|f| f.parent() == Some(dir.as_path())) {
            return Err(err(VfsErrorKind::NotEmpty, "remove_dir"));
        }
        if !s.dirs.remove(&dir) {
            return Err(err(VfsErrorKind::NotFound, "remove_dir"));
        }
        Ok(())
    }

    fn list_dir(
        &self,
        root: &PathBuf,
        dir: Option<RelPath<'_>>,
    ) -> Result<Vec<DirEntry>, VfsError> {
        let d = dir.map_or(root.clone(), |r| root.join(r.as_str()));
        Ok(self
            .state
            .borrow()
            .files
            .keys()
            .filter(|f| f.parent() == Some(d.as_path()))
            .map(|f| DirEntry {
                name: EntryName::from_os_bytes(name(f).as_bytes()),
                kind: EntryKind::File,
            })
            .collect())
    }

    fn read_at(&self, file: &MemFile, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut s = self.state.borrow_mut();
        s.log.reads.push((name(&file.path), offset, buf.len()));
        let b = s
            .files
            .get(&file.path)
            .ok_or(err(VfsErrorKind::NotFound, "read_at"))?;
        let off = (offset as usize).min(b.len());
        let n = (b.len() - off).min(buf.len());
        buf[..n].copy_from_slice(&b[off..off + n]);
        Ok(n)
    }

    fn read_exact_at(&self, file: &MemFile, offset: u64, buf: &mut [u8]) -> Result<(), VfsError> {
        if self.read_at(file, offset, buf)? == buf.len() {
            Ok(())
        } else {
            Err(err(VfsErrorKind::UnexpectedEof, "read_exact_at"))
        }
    }

    fn write_at(&self, file: &MemFile, offset: u64, buf: &[u8]) -> Result<(), VfsError> {
        let mut s = self.state.borrow_mut();
        if s.sealed.contains(&file.path) {
            return Err(err(VfsErrorKind::AccessDenied, "write_at"));
        }
        let b = s
            .files
            .get_mut(&file.path)
            .ok_or(err(VfsErrorKind::NotFound, "write_at"))?;
        let end = offset as usize + buf.len();
        if b.len() < end {
            b.resize(end, 0);
        }
        b[offset as usize..end].copy_from_slice(buf);
        Ok(())
    }

    fn sync(&self, _file: &MemFile, _kind: SyncKind) -> Result<(), DurabilityFailure> {
        Ok(())
    }

    fn sync_dir(
        &self,
        _root: &PathBuf,
        _dir: Option<RelPath<'_>>,
    ) -> Result<(), DurabilityFailure> {
        Ok(())
    }

    fn sync_group(
        &self,
        _members: &[GroupMember<'_, PathBuf, MemFile>],
    ) -> Result<(), DurabilityFailure> {
        Ok(())
    }

    fn fail_stop(&self, failure: DurabilityFailure) -> ! {
        panic!("fail_stop: {failure:?}")
    }

    fn create_extent(
        &self,
        _: &PathBuf,
        _: RelPath<'_>,
        _: u64,
        _: &StoreVolume,
    ) -> Result<MemFile, VfsError> {
        Err(err(VfsErrorKind::Unsupported, "create_extent"))
    }

    fn recycle_extent(&self, _: &MemFile, _: u64, _: &StoreVolume) -> Result<(), VfsError> {
        Err(err(VfsErrorKind::Unsupported, "recycle_extent"))
    }

    fn seal(&self, file: &MemFile) -> Result<(), VfsError> {
        self.state.borrow_mut().sealed.insert(file.path.clone());
        Ok(())
    }

    fn unlink(&self, root: &PathBuf, rel: RelPath<'_>, _retry: ShareRetry) -> Result<(), VfsError> {
        let path = root.join(rel.as_str());
        let mut s = self.state.borrow_mut();
        s.sealed.remove(&path);
        s.files
            .remove(&path)
            .map(|_| ())
            .ok_or(err(VfsErrorKind::NotFound, "unlink"))
    }

    fn rename_noreplace(
        &self,
        _: &PathBuf,
        _: RelPath<'_>,
        _: &PathBuf,
        _: RelPath<'_>,
        _: ShareRetry,
    ) -> Result<(), VfsError> {
        Err(err(VfsErrorKind::Unsupported, "rename_noreplace"))
    }

    fn rename_replace(
        &self,
        _: &PathBuf,
        _: RelPath<'_>,
        _: &PathBuf,
        _: RelPath<'_>,
        _: ShareRetry,
    ) -> Result<(), VfsError> {
        Err(err(VfsErrorKind::Unsupported, "rename_replace"))
    }

    fn swap_dirs(
        &self,
        _: &PathBuf,
        _: RelPath<'_>,
        _: &PathBuf,
        _: RelPath<'_>,
        _: ShareRetry,
    ) -> Result<SwapOutcome, VfsError> {
        Err(err(VfsErrorKind::Unsupported, "swap_dirs"))
    }

    fn swap_recover(
        &self,
        _: &PathBuf,
        _: RelPath<'_>,
        _: ShareRetry,
    ) -> Result<SwapRecovery, VfsError> {
        Err(err(VfsErrorKind::Unsupported, "swap_recover"))
    }

    fn file_size(&self, file: &MemFile) -> Result<u64, VfsError> {
        self.state
            .borrow()
            .files
            .get(&file.path)
            .map(|b| b.len() as u64)
            .ok_or(err(VfsErrorKind::NotFound, "file_size"))
    }

    fn identity(&self, _: &MemFile) -> Result<FileIdentity, VfsError> {
        Err(err(VfsErrorKind::Unsupported, "identity"))
    }

    fn root_identity(&self, _: &PathBuf) -> Result<FileIdentity, VfsError> {
        Err(err(VfsErrorKind::Unsupported, "root_identity"))
    }

    fn path_identity(&self, _: &PathBuf, _: RelPath<'_>) -> Result<FileIdentity, VfsError> {
        Err(err(VfsErrorKind::Unsupported, "path_identity"))
    }

    fn free_space(&self, _: &PathBuf) -> Result<FreeSpace, VfsError> {
        Err(err(VfsErrorKind::Unsupported, "free_space"))
    }

    fn advise_dontneed(&self, _: &MemFile, _: u64, _: u64) {}

    fn counters(&self) -> VfsCounters {
        VfsCounters::default()
    }
}

impl SealedMaps for MemFs {
    type Map = MemMap;

    fn map_sealed(
        &self,
        file: &MemFile,
        expected_len: u64,
        name_: RelPath<'_>,
    ) -> Result<MemMap, MapError> {
        if expected_len == 0 {
            return Err(MapError::Empty);
        }
        let mut s = self.state.borrow_mut();
        let b = s
            .files
            .get(&file.path)
            .cloned()
            .ok_or(MapError::Io(err(VfsErrorKind::NotFound, "map_sealed")))?;
        if b.len() as u64 != expected_len {
            return Err(MapError::SizeMismatch {
                expected: expected_len,
                actual: b.len() as u64,
            });
        }
        s.log.maps.push((name_.as_str().to_string(), expected_len));
        Ok(MemMap(Arc::new(b)))
    }

    fn advise(&self, _: &MemMap, _: u64, _: u64, _: Advice) {}
}
