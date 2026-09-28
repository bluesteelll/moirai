//! `swap_dirs` and `swap_recover` ([OS/fs §4.9]; [F15 §5.6]).
//!
//! The native form (Linux `RENAME_EXCHANGE`, macOS `RENAME_SWAP`, [OS/fs §4.9.1]) is one namespace operation, then
//! `sync_dir` of both parents. Windows always uses the emulated form, and so does a Linux or macOS world whose volume the
//! adversary declares without the exchange ([`crate::Site::SwapExchange`]). The emulated form (§4.9.2) writes the intent
//! file `<a>.swap` (§4.9.3, exact bytes, XXH3-64 checksum), then renames `A → T`, `B → A`, `T → B` and unlinks the
//! intent, with a `durable-name` after every step. Every step is a real call of the simulated `Vfs`, with its own
//! scheduling points and trace events, so a crash or a death lands between any two steps and inside any of them; the
//! crash states are then the prefixes of the steps (at most the last unsynced step lost, [F15 §5.6]). `swap_recover`
//! reads the intent and acts by the table of §4.9.4.
//!
//! The paths in the intent are the simulator's machine-local absolute form of the three names
//! ([`crate::namespace::Ns::abs_path`]): `/`-separated from the world root, a leading drive component upper-cased.
//! Failures of the `durable-name` steps are returned as `VfsError` ([OS/fs §4.9]'s signatures): the caller exits 7
//! through its error path, not `fail_stop`, so later calls of the process are not protocol violations.

use moirai_vfs::{
    Access, FileIdentity, OpenHint, OsCode, OsTag, RelPath, RootAccess, RootRole, ShareRetry,
    StoreFs, SwapOutcome, SwapRecovery, SyncKind, VfsError, VfsErrorKind,
};

use crate::adversary::Site;
use crate::namespace::NsOp;
use crate::trace::EventKind;
use crate::vfs::{
    Op, SimRoot, SimVfs, err, flag_bounded_retry, note_nonlazy_call, writable_volume,
};
use crate::world::{CallKind, Ctx, error_code};
use crate::xxh3::xxh3_64;

/// The intent's magic ([OS/fs §4.9.3]).
const MAGIC: [u8; 4] = *b"MSWP";
/// The header length.
const HEADER: usize = 64;
/// The longest path an intent holds.
const MAX_PATH: usize = 4096;

/// The content of a swap intent ([OS/fs §4.9.3]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Intent {
    pub(crate) a_id: FileIdentity,
    pub(crate) b_id: FileIdentity,
    pub(crate) a_path: String,
    pub(crate) b_path: String,
    pub(crate) t_path: String,
}

fn id_bytes(id: &FileIdentity) -> [u8; 24] {
    let mut b = [0u8; 24];
    b[..8].copy_from_slice(&id.volume.to_le_bytes());
    b[8..].copy_from_slice(&id.file);
    b
}

fn id_from(b: &[u8]) -> FileIdentity {
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[..8]);
    let mut file = [0u8; 16];
    file.copy_from_slice(&b[8..24]);
    FileIdentity {
        volume: u64::from_le_bytes(v),
        file,
    }
}

fn u16_at(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

impl Intent {
    /// The file's bytes: the 64-byte header, the three paths, zero padding to a multiple of 8, and the XXH3-64 of all
    /// that. The paths are 1–4096 bytes each (checked by the caller).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let paths = [&self.a_path, &self.b_path, &self.t_path];
        let p = HEADER + paths.iter().map(|s| s.len()).sum::<usize>();
        let pad = (8 - p % 8) % 8;
        let mut b = Vec::with_capacity(p + pad + 8);
        b.extend_from_slice(&MAGIC);
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&id_bytes(&self.a_id));
        b.extend_from_slice(&id_bytes(&self.b_id));
        for s in paths {
            b.extend_from_slice(&(s.len() as u16).to_le_bytes());
        }
        b.extend_from_slice(&0u16.to_le_bytes());
        for s in paths {
            b.extend_from_slice(s.as_bytes());
        }
        b.resize(p + pad, 0);
        let sum = xxh3_64(&b);
        b.extend_from_slice(&sum.to_le_bytes());
        b
    }

    /// The intent in `b`, if the length, the magic, `version = 1`, the reserved fields, the padding, UTF-8 paths and the
    /// checksum all check.
    pub(crate) fn decode(b: &[u8]) -> Option<Intent> {
        if b.len() < HEADER + 8 || b[..4] != MAGIC || u16_at(b, 4) != 1 || u16_at(b, 6) != 0 {
            return None;
        }
        let lens = [u16_at(b, 56), u16_at(b, 58), u16_at(b, 60)];
        if u16_at(b, 62) != 0 || lens.iter().any(|&l| l == 0 || l > MAX_PATH) {
            return None;
        }
        let p = HEADER + lens.iter().sum::<usize>();
        let pad = (8 - p % 8) % 8;
        if b.len() != p + pad + 8 || b[p..p + pad].iter().any(|&x| x != 0) {
            return None;
        }
        let mut sum = [0u8; 8];
        sum.copy_from_slice(&b[p + pad..]);
        if xxh3_64(&b[..p + pad]) != u64::from_le_bytes(sum) {
            return None;
        }
        let mut at = HEADER;
        let mut path = |l: usize| {
            let s = core::str::from_utf8(&b[at..at + l]).ok().map(str::to_owned);
            at += l;
            s
        };
        Some(Intent {
            a_id: id_from(&b[8..32]),
            b_id: id_from(&b[32..56]),
            a_path: path(lens[0])?,
            b_path: path(lens[1])?,
            t_path: path(lens[2])?,
        })
    }
}

/// The longest intent file.
const MAX_INTENT: u64 = (HEADER + 3 * MAX_PATH + 7 + 8) as u64;

/// The names the emulated form uses in `a_parent`: the intent `<a>.swap` and the temporary `<a>.swap-old`.
fn side_names(a: &str) -> (String, String) {
    (format!("{a}.swap"), format!("{a}.swap-old"))
}

fn rel(s: &str) -> Result<RelPath<'_>, VfsError> {
    RelPath::new(s).map_err(|_| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "swap_dirs"))
}

/// What the call's start decided.
enum Form {
    /// The native exchange was performed; its `durable-name` steps follow.
    Native,
    /// The emulated steps follow, with these paths of `A`, `B` and `T`.
    Emulated(String, String, String),
}

/// `swap_dirs` ([OS/fs §4.9]).
pub(crate) fn swap_dirs(
    v: &SimVfs,
    a_parent: &SimRoot,
    a: RelPath<'_>,
    b_parent: &SimRoot,
    b: RelPath<'_>,
    retry: ShareRetry,
) -> Result<SwapOutcome, VfsError> {
    let mut ctx = v.enter(CallKind::SwapDirs, a_parent.node);
    let form = start(&mut ctx, a_parent, a, b_parent, b, retry);
    let r = match form {
        Err(e) => Err(e),
        Ok(Form::Native) => {
            // [OS/fs §4.9.1]: then `sync_dir` of both parents, each a call of its own.
            drop(ctx);
            let r = sync_both(v, a_parent, b_parent).map(|()| SwapOutcome::Exchanged);
            ctx = Ctx::quiet(&v.sh, v.proc);
            r
        }
        Ok(Form::Emulated(a_path, b_path, t_path)) => {
            drop(ctx);
            let r = emulate(v, a_parent, a, b_parent, b, retry, [a_path, b_path, t_path]);
            ctx = Ctx::quiet(&v.sh, v.proc);
            r
        }
    };
    let code = r.as_ref().map_or_else(|e| error_code(e.kind), |_| 0);
    ctx.ret_code(CallKind::SwapDirs, a_parent.node, code);
    r
}

/// The call's start: the checks, the form, and the native exchange itself.
fn start(
    ctx: &mut Ctx<'_>,
    a_parent: &SimRoot,
    a: RelPath<'_>,
    b_parent: &SimRoot,
    b: RelPath<'_>,
    retry: ShareRetry,
) -> Result<Form, VfsError> {
    let proc = ctx.proc;
    let st = ctx.st();
    if a.segments().count() != 1 || b.segments().count() != 1 {
        return Err(err(st, VfsErrorKind::InvalidName, Op::Rename));
    }
    if a_parent.access == RootAccess::Read || b_parent.access == RootAccess::Read {
        return Err(err(st, VfsErrorKind::AccessDenied, Op::Rename));
    }
    note_nonlazy_call(st, proc, a_parent.node);
    flag_bounded_retry(st, proc, retry, a_parent.node);
    let an = st.k.ns.child(a_parent.node, a.as_str());
    let bn = st.k.ns.child(b_parent.node, b.as_str());
    let (Some(an), Some(bn)) = (an, bn) else {
        return Err(err(st, VfsErrorKind::NotFound, Op::Rename));
    };
    if !st.k.ns.node(an).is_dir() || !st.k.ns.node(bn).is_dir() {
        return Err(err(st, VfsErrorKind::NotFound, Op::Rename));
    }
    if st.k.ns.node(an).vol != st.k.ns.node(bn).vol {
        return Err(err(st, VfsErrorKind::CrossDevice, Op::Rename));
    }
    writable_volume(st, an, Op::Rename)?;
    let native = matches!(st.cfg.os, OsTag::Linux | OsTag::MacOs)
        && st.pick(Site::SwapExchange, proc, an, 0, 2) == 0;
    if native {
        crate::vfs::attempt(ctx, &[an, bn], retry, Op::Rename)?;
        let st = ctx.st();
        let id = st.k.ns.push(NsOp::Exchange {
            a: (a_parent.node, a.as_str().to_owned()),
            b: (b_parent.node, b.as_str().to_owned()),
            a_node: an,
            b_node: bn,
        });
        st.ev(EventKind::NsOp, None, proc, id, 4, an);
        st.k.procs[proc as usize].counters.renames += 1;
        return Ok(Form::Native);
    }
    // §4.9.2 step 1: an intent or a temporary name left by an earlier swap is `doctor`'s.
    let (i_name, t_name) = side_names(a.as_str());
    if rel(&i_name).is_err() || rel(&t_name).is_err() {
        return Err(err(st, VfsErrorKind::InvalidName, Op::Rename));
    }
    if st.k.ns.child(a_parent.node, &i_name).is_some()
        || st.k.ns.child(a_parent.node, &t_name).is_some()
    {
        return Err(err(st, VfsErrorKind::AlreadyExists, Op::Rename));
    }
    let (Some(ap), Some(bp)) = (
        st.k.ns.abs_path(a_parent.node),
        st.k.ns.abs_path(b_parent.node),
    ) else {
        return Err(err(st, VfsErrorKind::NotFound, Op::Rename));
    };
    let join = |p: &str, n: &str| {
        if p.ends_with('/') {
            format!("{p}{n}")
        } else {
            format!("{p}/{n}")
        }
    };
    let paths = (
        join(&ap, a.as_str()),
        join(&bp, b.as_str()),
        join(&ap, &t_name),
    );
    if [&paths.0, &paths.1, &paths.2]
        .iter()
        .any(|p| p.len() > MAX_PATH)
    {
        return Err(err(st, VfsErrorKind::InvalidName, Op::Rename));
    }
    Ok(Form::Emulated(paths.0, paths.1, paths.2))
}

/// Maps an embedded `durable-name` or `durable+meta` failure to the call's error.
fn embedded(r: Result<(), moirai_vfs::DurabilityFailure>) -> Result<(), VfsError> {
    r.map_err(|f| VfsError::new(f.kind, f.os, f.call))
}

/// `durable-name` on `a` and, if it is another directory, on `b`.
fn sync_both(v: &SimVfs, a: &SimRoot, b: &SimRoot) -> Result<(), VfsError> {
    embedded(v.sync_dir_op(a, None, true))?;
    if b.node != a.node {
        embedded(v.sync_dir_op(b, None, true))?;
    }
    Ok(())
}

/// [OS/fs §4.9.2] steps 2–7, each a real call.
fn emulate(
    v: &SimVfs,
    a_parent: &SimRoot,
    a: RelPath<'_>,
    b_parent: &SimRoot,
    b: RelPath<'_>,
    retry: ShareRetry,
    [a_path, b_path, t_path]: [String; 3],
) -> Result<SwapOutcome, VfsError> {
    let (i_name, t_name) = side_names(a.as_str());
    let i = rel(&i_name)?;
    let t = rel(&t_name)?;
    // 2. The identities.
    let a_id = v.path_identity(a_parent, a)?;
    let b_id = v.path_identity(b_parent, b)?;
    // 3. The intent, durable with its name.
    let bytes = Intent {
        a_id,
        b_id,
        a_path,
        b_path,
        t_path,
    }
    .encode();
    let f = v.create_new(a_parent, i)?;
    v.write_at(&f, 0, &bytes)?;
    embedded(v.sync_op(&f, SyncKind::DataAndMeta, true))?;
    drop(f);
    embedded(v.sync_dir_op(a_parent, None, true))?;
    // 4. A → T.
    v.rename_op(a_parent, a, a_parent, t, retry, false, false)?;
    embedded(v.sync_dir_op(a_parent, None, true))?;
    // 5. B → A.
    v.rename_op(b_parent, b, a_parent, a, retry, false, false)?;
    sync_both(v, b_parent, a_parent)?;
    // 6. T → B.
    v.rename_op(a_parent, t, b_parent, b, retry, false, false)?;
    sync_both(v, a_parent, b_parent)?;
    // 7. The intent goes.
    v.unlink_op(a_parent, i, retry, false)?;
    embedded(v.sync_dir_op(a_parent, None, true))?;
    Ok(SwapOutcome::TwoRenames)
}

/// `swap_recover` ([OS/fs §4.9.4]).
pub(crate) fn swap_recover(
    v: &SimVfs,
    a_parent: &SimRoot,
    a: RelPath<'_>,
    retry: ShareRetry,
) -> Result<SwapRecovery, VfsError> {
    let mut ctx = v.enter(CallKind::SwapRecover, a_parent.node);
    let checked = {
        let proc = ctx.proc;
        let st = ctx.st();
        flag_bounded_retry(st, proc, retry, a_parent.node);
        if a.segments().count() != 1 {
            Err(err(st, VfsErrorKind::InvalidName, Op::Rename))
        } else {
            Ok(())
        }
    };
    let r = match checked {
        Err(e) => Err(e),
        Ok(()) => {
            drop(ctx);
            let r = recover(v, a_parent, a, retry);
            ctx = Ctx::quiet(&v.sh, v.proc);
            r
        }
    };
    let code = r.as_ref().map_or_else(|e| error_code(e.kind), |_| 0);
    ctx.ret_code(CallKind::SwapRecover, a_parent.node, code);
    r
}

fn unrecognised(what: &'static str) -> VfsError {
    VfsError::new(VfsErrorKind::Io, OsCode::NONE, what)
}

/// The recovery itself: every read, rename and unlink a real call.
fn recover(
    v: &SimVfs,
    a_parent: &SimRoot,
    a: RelPath<'_>,
    retry: ShareRetry,
) -> Result<SwapRecovery, VfsError> {
    let (i_name, _) = side_names(a.as_str());
    let i = rel(&i_name)?;
    let f = match v.open(a_parent, i, Access::Read, OpenHint::Normal) {
        Ok(f) => f,
        Err(e) if e.kind == VfsErrorKind::NotFound => return Ok(SwapRecovery::NoIntent),
        Err(e) => return Err(e),
    };
    let n = v.file_size(&f)?;
    if n > MAX_INTENT {
        return Err(unrecognised("swap intent unreadable"));
    }
    let mut buf = vec![0u8; n as usize];
    v.read_exact_at(&f, 0, &mut buf)?;
    drop(f);
    let intent = Intent::decode(&buf).ok_or_else(|| unrecognised("swap intent unreadable"))?;
    // The three names, as the intent records them.
    let locate = |path: &str| -> Option<(SimRoot, String)> {
        let g = v.sh.lock();
        let (dir, name) = g.k.ns.resolve_parent(path).ok()?;
        Some((
            SimRoot {
                node: dir,
                role: RootRole::Other,
                access: a_parent.access,
            },
            name,
        ))
    };
    let (la, lb, lt) = (
        locate(&intent.a_path),
        locate(&intent.b_path),
        locate(&intent.t_path),
    );
    let identity = |l: &Option<(SimRoot, String)>| -> Result<Option<FileIdentity>, VfsError> {
        let Some((root, name)) = l else {
            return Ok(None);
        };
        match v.path_identity(root, rel(name)?) {
            Ok(id) => Ok(Some(id)),
            Err(e) if e.kind == VfsErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    };
    let (ia, ib, it) = (identity(&la)?, identity(&lb)?, identity(&lt)?);
    let (aid, bid) = (Some(intent.a_id), Some(intent.b_id));
    let result = if ia == aid && ib == bid && it.is_none() {
        SwapRecovery::NothingDone
    } else if ia.is_none() && ib == bid && it == aid {
        let (Some((ra, na)), Some((rt, nt))) = (&la, &lt) else {
            return Err(unrecognised("swap state unrecognised"));
        };
        v.rename_op(rt, rel(nt)?, ra, rel(na)?, retry, false, false)?;
        sync_both(v, rt, ra)?;
        SwapRecovery::RolledBack
    } else if ia == bid && ib.is_none() && it == aid {
        let (Some((rb, nb)), Some((rt, nt))) = (&lb, &lt) else {
            return Err(unrecognised("swap state unrecognised"));
        };
        v.rename_op(rt, rel(nt)?, rb, rel(nb)?, retry, false, false)?;
        sync_both(v, rt, rb)?;
        SwapRecovery::Completed
    } else if ia == bid && ib == aid && it.is_none() {
        SwapRecovery::Completed
    } else {
        return Err(unrecognised("swap state unrecognised"));
    };
    v.unlink_op(a_parent, i, retry, false)?;
    embedded(v.sync_dir_op(a_parent, None, true))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> FileIdentity {
        FileIdentity {
            volume: 0x5349_4D00_0000_0000,
            file: [n; 16],
        }
    }

    #[test]
    fn the_intent_round_trips_and_rejects_every_damage() {
        let i = Intent {
            a_id: id(1),
            b_id: id(2),
            a_path: "C:/sim/store".into(),
            b_path: "C:/sim/restored".into(),
            t_path: "C:/sim/store.swap-old".into(),
        };
        let b = i.encode();
        assert_eq!(&b[..4], b"MSWP");
        assert_eq!(b.len() % 8, 0);
        assert_eq!(u16_at(&b, 56), 12);
        let p = HEADER + 12 + 15 + 21;
        assert_eq!(b.len(), p + (8 - p % 8) % 8 + 8);
        assert_eq!(Intent::decode(&b), Some(i));
        for at in [0, 4, 6, 8, 40, 56, 62, 64, b.len() - 9, b.len() - 1] {
            let mut d = b.clone();
            d[at] ^= 1;
            assert_eq!(Intent::decode(&d), None, "byte {at}");
        }
        assert_eq!(Intent::decode(&b[..b.len() - 1]), None);
        assert_eq!(Intent::decode(&[]), None);
    }
}
