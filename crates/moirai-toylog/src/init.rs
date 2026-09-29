//! `init` ([F16] P-88, P-24, [F02 §2.4], [F04 §10]): the store directory, `tmp/`, the environment probe, `LOCK`,
//! `config`, log.1 with its epoch-start group and the group that creates `main`, and `HEAD` last through
//! `tmp/head.<nonce>`. `init` takes no lock byte: until `HEAD` is named no process can discover the store.

use std::path::Path;

use moirai_vfs::{
    BootIdentity, Entropy, ProbeOutcome, RelPath, RootRole, ShareRetry, SyncKind, Vfs, VfsError,
    VfsErrorKind, hlc_next,
};

use crate::bugs::Bug;
use crate::codec::{Writer, hash64};
use crate::config::Config;
use crate::format::{
    Counters, ExtentHeadRec, HEAD_GROUP, REASON_CREATE, Rec, RefEntry, RefTableRec, RefUpdateRec,
    encode_group, kind,
};
use crate::head::{HEAD_LEN, SLOT_LEN, Slot};
use crate::state::MAIN;
use crate::store::{ToyError, log_name, rel};

/// The length of `LOCK` ([F03 §2.1]).
pub const LOCK_LEN: usize = 36_864;

/// `LockHdr` ([F03 §4.1]): 64 bytes at offset 0.
pub fn lock_header(created_hlc: u64) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0..4].copy_from_slice(b"MLCK");
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[8..10].copy_from_slice(&256u16.to_le_bytes());
    b[10..12].copy_from_slice(&128u16.to_le_bytes());
    b[12..20].copy_from_slice(&moirai_vfs::ROLE_BASE.to_le_bytes());
    b[20..28].copy_from_slice(&moirai_vfs::SLOT_BASE.to_le_bytes());
    b[28..36].copy_from_slice(&created_hlc.to_le_bytes());
    let sum = hash64(&b[..56]);
    b[56..64].copy_from_slice(&sum.to_le_bytes());
    b
}

/// A random non-zero `u64` from the seam's source ([OS/README §4.6]; a zero draw is drawn again).
pub(crate) fn nonzero_u64<V: Entropy>(vfs: &V) -> u64 {
    loop {
        let mut b = [0u8; 8];
        vfs.fill_random(&mut b);
        let v = u64::from_le_bytes(b);
        if v != 0 {
            return v;
        }
    }
}

fn err(e: VfsError) -> ToyError {
    if matches!(
        e.kind,
        VfsErrorKind::DiskFull | VfsErrorKind::InsufficientSpace
    ) {
        ToyError::DiskFull
    } else {
        ToyError::Io(e)
    }
}

/// Creates a store in the new directory `dir` ([F16] P-88). P-24's seeded bug renames `HEAD` into place before log.1 is
/// durable; P-88's leaves `next_ref_id` at 0 after `main` took ref id 0.
pub fn init<V: Vfs>(vfs: &V, dir: &Path, cfg: &Config) -> Result<(), ToyError> {
    let bugs = cfg.bugs;
    let fail = |f: moirai_vfs::DurabilityFailure| -> ! { vfs.fail_stop(f) };
    // Step 1: the store directory (its name made durable on the parent) and tmp/.
    let root = vfs.create_root(dir, RootRole::Store).map_err(err)?;
    vfs.create_dir(&root, RelPath::literal("tmp"))
        .map_err(err)?;
    if let Err(f) = vfs.sync_dir(&root, None) {
        fail(f);
    }
    // Step 2: the environment probe (P-94).
    match vfs.probe_store(&root).map_err(err)? {
        ProbeOutcome::Admitted(_) => {}
        ProbeOutcome::Refused(r) => return Err(ToyError::Location(format!("{r:?}"))),
    }
    let wall = vfs.wall_ms();
    // Step 3: LOCK, all 36,864 bytes, durable+meta, then durable-name.
    let lock = vfs
        .create_new(&root, RelPath::literal("LOCK"))
        .map_err(err)?;
    let mut lb = vec![0u8; LOCK_LEN];
    lb[..64].copy_from_slice(&lock_header((wall.max(0) as u64) << 16));
    vfs.write_at(&lock, 0, &lb).map_err(err)?;
    if let Err(f) = vfs.sync(&lock, SyncKind::DataAndMeta) {
        fail(f);
    }
    if let Err(f) = vfs.sync_dir(&root, None) {
        fail(f);
    }
    // Step 4: config through tmp/config.<nonce>.
    let tmp_cfg = rel(&format!("tmp/config.{}", nonzero_u64(vfs)));
    let cf = vfs.create_new(&root, tmp_cfg.as_rel_path()).map_err(err)?;
    vfs.write_at(&cf, 0, b"# moirai store configuration\n")
        .map_err(err)?;
    if let Err(f) = vfs.sync(&cf, SyncKind::DataAndMeta) {
        fail(f);
    }
    drop(cf);
    vfs.rename_noreplace(
        &root,
        tmp_cfg.as_rel_path(),
        &root,
        RelPath::literal("config"),
        ShareRetry::None,
    )
    .map_err(err)?;
    if let Err(f) = vfs.sync_dir(&root, Some(RelPath::literal("tmp"))) {
        fail(f);
    }
    if let Err(f) = vfs.sync_dir(&root, None) {
        fail(f);
    }
    // Step 5: log.1 prepared by P-8, the epoch-start group and the group that creates main.
    let e = cfg.extent_bytes;
    let vol = match vfs
        .classify(&root, moirai_vfs::ClassifyDepth::Open)
        .map_err(err)?
    {
        moirai_vfs::Classification::Local(v) => v,
        moirai_vfs::Classification::Refused(r) => return Err(ToyError::Location(format!("{r:?}"))),
    };
    let log = vfs
        .create_extent(&root, log_name(1).as_rel_path(), e, &vol)
        .map_err(err)?;
    let prepared = |vfs: &V| {
        if let Err(f) = vfs.sync(&log, SyncKind::DataAndMeta) {
            vfs.fail_stop(f);
        }
        if let Err(f) = vfs.sync_dir(&root, None) {
            vfs.fail_stop(f);
        }
    };
    if !bugs.on(Bug::P24HeadBeforeLogDurable) {
        prepared(vfs);
    }
    let epoch = nonzero_u64(vfs);
    let mut store_id = [0u8; 16];
    while store_id == [0; 16] {
        vfs.fill_random(&mut store_id);
    }
    let boot_id = match vfs.boot_identity() {
        BootIdentity::Known(b) => b.0,
        BootIdentity::Unknown(_) => [0; 16],
    };
    let img = image(cfg, epoch, store_id, boot_id, wall);
    vfs.write_at(&log, 0, &img.log).map_err(err)?;
    let log_durable = |vfs: &V| {
        if let Err(f) = vfs.sync(&log, SyncKind::Data) {
            vfs.fail_stop(f);
        }
    };
    if !bugs.on(Bug::P24HeadBeforeLogDurable) {
        log_durable(vfs);
    }
    // Step 6: HEAD through tmp/head.<nonce>, both slots, durable+meta, rename, durable-name on tmp/ and the store.
    let tmp_head = rel(&format!("tmp/head.{}", nonzero_u64(vfs)));
    let hf = vfs.create_new(&root, tmp_head.as_rel_path()).map_err(err)?;
    vfs.write_at(&hf, 0, &img.head).map_err(err)?;
    if let Err(f) = vfs.sync(&hf, SyncKind::DataAndMeta) {
        fail(f);
    }
    drop(hf);
    vfs.rename_noreplace(
        &root,
        tmp_head.as_rel_path(),
        &root,
        RelPath::literal("HEAD"),
        ShareRetry::None,
    )
    .map_err(err)?;
    if let Err(f) = vfs.sync_dir(&root, Some(RelPath::literal("tmp"))) {
        fail(f);
    }
    if let Err(f) = vfs.sync_dir(&root, None) {
        fail(f);
    }
    if bugs.on(Bug::P24HeadBeforeLogDurable) {
        prepared(vfs);
        log_durable(vfs);
    }
    Ok(())
}

/// The bytes `init` writes, computed without any call: the groups at the start of log.1 (the epoch-start group and the
/// group that creates `main`), both `HEAD` slots, and `LOCK` ([F16] P-88 steps 3, 5, 6; [F04 §10]). A harness that needs
/// a store to exist before its scenario puts these files in place.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image {
    /// The first bytes of log.1 (the rest of the extent is zero).
    pub log: Vec<u8>,
    /// `HEAD`: both slots.
    pub head: Vec<u8>,
    /// `LOCK`: `LockHdr` and zeros.
    pub lock: Vec<u8>,
}

/// The image of a new store with `epoch`, `store_id` and the creating process's `boot_id` (zero in Unknown-boot mode), at
/// wall-clock `wall_ms`. P-88's seeded bug leaves `next_ref_id` at 0.
pub fn image(
    cfg: &Config,
    epoch: u64,
    store_id: [u8; 16],
    boot_id: [u8; 16],
    wall_ms: i64,
) -> Image {
    let bugs = cfg.bugs;
    let init = cfg.init_params(store_id);
    let head_rec = ExtentHeadRec {
        epoch_lsn: 0,
        chain_in: hash64(&epoch.to_le_bytes()),
        init,
        project_oid_algo: 1,
        hflags: 0,
        counters: Counters::EMPTY,
    };
    let mut bytes = Vec::with_capacity(512);
    let seed = encode_group(
        &[Rec::new(kind::EXTENT_HEAD, head_rec.encode(), bugs)],
        0,
        epoch,
        head_rec.chain_in,
        &mut bytes,
    );
    let hlc = hlc_next(wall_ms, 0);
    let update = Rec::new(
        kind::REF_UPDATE,
        RefUpdateRec {
            reason: REASON_CREATE,
            ref_id: 0,
            op: 0,
            hlc,
            old: 0,
            new: 0,
        }
        .encode(),
        bugs,
    );
    let refs_lsn = HEAD_GROUP + update.len(false);
    let table = Rec::new(
        kind::REF_TABLE,
        RefTableRec {
            entries: vec![RefEntry {
                ref_id: 0,
                name: MAIN,
                rkind: 1,
                ..RefEntry::default()
            }],
        }
        .encode(),
        bugs,
    );
    encode_group(&[update, table], HEAD_GROUP, epoch, seed, &mut bytes);
    let end = bytes.len() as u64;
    let mut slot = Slot {
        format: 1,
        flags: 0,
        slot_seq: 1,
        epoch,
        committed_lsn: end,
        durable_lsn: end,
        boot_id,
        config_gen: 0,
        checkpoint_lsn: 0,
        counters: Counters {
            next_ref_id: if bugs.on(Bug::P88InitKeepsRefIdZero) {
                0
            } else {
                1
            },
            hlc_seq: hlc,
            ..Counters::EMPTY
        },
        active_log: 1,
        segments: Vec::new(),
        refs_lsn,
        pins_lsn: 0,
        heads_lsn: 0,
        markers_lsn: 0,
        seq_ring: [(0, 0); 32],
        init,
        epoch_lsn: 0,
        project_oid_algo: 1,
    };
    let mut hb = Writer::with_capacity(HEAD_LEN);
    hb.bytes(&slot.to_bytes());
    slot.slot_seq = 2;
    hb.bytes(&slot.to_bytes());
    debug_assert_eq!(hb.buf.len(), 2 * SLOT_LEN);
    let mut lock = vec![0u8; LOCK_LEN];
    lock[..64].copy_from_slice(&lock_header((wall_ms.max(0) as u64) << 16));
    Image {
        log: bytes,
        head: hb.buf,
        lock,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bugs::Bugs;
    use crate::format::validate_record;
    use crate::head::{Choice, choose};

    fn image_of(bugs: Bugs) -> Image {
        let cfg = Config::test_profile().with_bugs(bugs);
        image(&cfg, 0x1234, [9; 16], [3; 16], 1_790_000_000_000)
    }

    #[test]
    fn the_image_holds_two_equal_valid_slots_and_two_valid_groups() {
        let img = image_of(Bugs::NONE);
        assert_eq!(img.head.len(), HEAD_LEN);
        assert_eq!(img.lock.len(), LOCK_LEN);
        let Choice::Newest(s, which) = choose(&img.head, false) else {
            panic!("the image's HEAD has no valid slot");
        };
        assert_eq!(which, 1, "slot B carries slot_seq 2");
        assert_eq!(s.slot_seq, 2);
        assert_eq!(s.committed_lsn, img.log.len() as u64);
        assert_eq!(s.durable_lsn, s.committed_lsn);
        assert_eq!(s.boot_id, [3; 16]);
        assert_eq!(s.counters.next_ref_id, 1, "main took ref id 0 ([F16] P-88)");

        // Both groups validate at their positions, the head at 0 and main's group after it.
        let e = s.init.log_extent_bytes;
        let mut ext = img.log.clone();
        ext.resize(e as usize, 0);
        let (h, end, len) = validate_record(&ext, 0, e, 0, 0x1234, Bugs::NONE).unwrap();
        assert!(end && h.kind == kind::EXTENT_HEAD && len == HEAD_GROUP);
        let (r, end, rlen) =
            validate_record(&ext, HEAD_GROUP as usize, e, HEAD_GROUP, 0x1234, Bugs::NONE).unwrap();
        assert!(!end && r.kind == kind::REF_UPDATE);
        assert_eq!(
            s.refs_lsn,
            HEAD_GROUP + rlen,
            "refs_lsn names the RefTable record"
        );
        // LockHdr: magic and checksum.
        assert_eq!(&img.lock[..4], b"MLCK");
        assert_eq!(
            u64::from_le_bytes(img.lock[56..64].try_into().unwrap_or([0; 8])),
            hash64(&img.lock[..56])
        );
    }

    #[test]
    fn p88_keeps_next_ref_id_zero() {
        let img = image_of(Bugs::only(Bug::P88InitKeepsRefIdZero));
        let Choice::Newest(s, _) = choose(&img.head, false) else {
            panic!("the image's HEAD has no valid slot");
        };
        assert_eq!(s.counters.next_ref_id, 0);
    }

    #[test]
    fn random_values_are_never_zero() {
        struct Zeros(std::cell::Cell<u32>);
        impl Entropy for Zeros {
            fn fill_random(&self, buf: &mut [u8]) {
                let n = self.0.get();
                self.0.set(n + 1);
                buf.fill(if n < 3 { 0 } else { 7 });
            }
        }
        let z = Zeros(std::cell::Cell::new(0));
        assert_eq!(nonzero_u64(&z), u64::from_le_bytes([7; 8]));
        assert_eq!(z.0.get(), 4);
    }
}
