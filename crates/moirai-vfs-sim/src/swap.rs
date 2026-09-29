//! `swap_dirs` and `swap_recover` ([OS/fs §4.9]; [F15 §5.6]).
//!
//! The native form (Linux `RENAME_EXCHANGE`, macOS `RENAME_SWAP`, [OS/fs §4.9.1]) is one namespace operation, then
//! `sync_dir` of both parents. Windows always uses the emulated form, and so does a Linux or macOS world whose volume the
//! adversary declares without the exchange ([`crate::Site::SwapExchange`]). The emulated form (§4.9.2) writes the intent
//! file `<a>.swap` (§4.9.3, exact bytes through [`SwapIntent`], the codec the OS layer shares), then renames `A → T`,
//! `B → A`, `T → B` and unlinks the intent, with a `durable-name` after every step. Every step is a real call of the
//! simulated `Vfs`, with its own
//! scheduling points and trace events, so a crash or a death lands between any two steps and inside any of them; the
//! crash states are then the prefixes of the steps (at most the last unsynced step lost, [F15 §5.6]). `swap_recover`
//! reads the intent and acts by the table of §4.9.4.
//!
//! The paths in the intent are the simulator's machine-local absolute form of the three names
//! ([`crate::namespace::Ns::abs_path`]): `/`-separated from the world root, a leading drive component upper-cased.
//! A failed flush embedded in a step is returned as `VfsError` of kind `FlushFailed` with the flush's code and call
//! ([OS/fs §4.1, §4.9], spec sync 2a) and leaves the intent for `swap_recover`; once the call has returned it, every
//! further write, flush, create or namespace call of the process is a protocol violation ([F15 §3.13]).

use moirai_vfs::{
    Access, FileIdentity, OpenHint, OsCode, OsTag, RelPath, RootAccess, RootRole, ShareRetry,
    StoreFs, SwapIntent, SwapOutcome, SwapRecovery, SyncKind, VfsError, VfsErrorKind,
};

use crate::adversary::Site;
use crate::namespace::NsOp;
use crate::trace::EventKind;
use crate::vfs::{
    Op, SimRoot, SimVfs, embedded_flush_ends, err, flag_bounded_retry, note_nonlazy_call,
    writable_volume,
};
use crate::world::{CallKind, Ctx, error_code};

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
    embedded_flush_ends(&mut ctx, &r);
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
    let (i_name, t_name) = SwapIntent::side_names(a.as_str());
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
        .any(|p| !SwapIntent::path_fits(p))
    {
        return Err(err(st, VfsErrorKind::InvalidName, Op::Rename));
    }
    Ok(Form::Emulated(paths.0, paths.1, paths.2))
}

/// Maps an embedded `durable-name` or `durable+meta` failure to the call's error: `FlushFailed` with the flush's code
/// and call ([OS/fs §4.1]).
fn embedded(r: Result<(), moirai_vfs::DurabilityFailure>) -> Result<(), VfsError> {
    r.map_err(|f| f.embedded())
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
    let (i_name, t_name) = SwapIntent::side_names(a.as_str());
    let i = rel(&i_name)?;
    let t = rel(&t_name)?;
    // 2. The identities.
    let a_id = v.path_identity(a_parent, a)?;
    let b_id = v.path_identity(b_parent, b)?;
    // 3. The intent, durable with its name.
    let bytes = SwapIntent {
        a_id,
        b_id,
        a_path,
        b_path,
        t_path,
    }
    .encode()
    .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "swap_dirs"))?;
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
    embedded_flush_ends(&mut ctx, &r);
    let code = r.as_ref().map_or_else(|e| error_code(e.kind), |_| 0);
    ctx.ret_code(CallKind::SwapRecover, a_parent.node, code);
    r
}

fn unrecognised(what: &'static str) -> VfsError {
    VfsError::new(VfsErrorKind::Io, OsCode::NONE, what)
}

/// Whether `rel` names an object in `root` (`NotFound` = absent; any other error is the call's).
fn present(v: &SimVfs, root: &SimRoot, rel: RelPath<'_>) -> Result<bool, VfsError> {
    match v.path_identity(root, rel) {
        Ok(_) => Ok(true),
        Err(e) if e.kind == VfsErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// The recovery itself: every read, rename and unlink a real call.
fn recover(
    v: &SimVfs,
    a_parent: &SimRoot,
    a: RelPath<'_>,
    retry: ShareRetry,
) -> Result<SwapRecovery, VfsError> {
    let (i_name, t_name) = SwapIntent::side_names(a.as_str());
    let i = rel(&i_name)?;
    let f = match v.open(a_parent, i, Access::Read, OpenHint::Normal) {
        Ok(f) => f,
        Err(e) if e.kind == VfsErrorKind::NotFound => return Ok(SwapRecovery::NoIntent),
        Err(e) => return Err(e),
    };
    // A read that returns an error is not an unreadable intent: the recovery fails with it and changes nothing
    // ([OS/fs §4.9.3]).
    let n = v.file_size(&f)?;
    let intent = if n > SwapIntent::MAX_LEN {
        None
    } else {
        let mut buf = vec![0u8; n as usize];
        v.read_exact_at(&f, 0, &mut buf)?;
        SwapIntent::decode(&buf)
    };
    drop(f);
    let Some(intent) = intent else {
        // [OS/fs §4.9.4] row "`I` unreadable": its write in step 3 never completed (a crash, or a failed flush), so
        // nothing was renamed, since step 4 starts only after `I` is durable. With `A` present and `T` absent the intent
        // is removed. `B` is named only inside the intent, so its presence cannot be read here; the soundness argument
        // needs only that step 4 never started, which `A` present and `T` absent confirm.
        let t = rel(&t_name)?;
        if present(v, a_parent, a)? && !present(v, a_parent, t)? {
            v.unlink_op(a_parent, i, retry, false)?;
            embedded(v.sync_dir_op(a_parent, None, true))?;
            return Ok(SwapRecovery::NothingDone);
        }
        return Err(unrecognised("swap intent unreadable"));
    };
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
