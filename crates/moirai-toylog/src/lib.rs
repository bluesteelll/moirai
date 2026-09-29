//! The toy log, generic over `V: Vfs`: the product's `RecHdr` and chained groups, leaderless group commit through the
//! writer and flush bytes (phases 2a and 2b), the two-slot `HEAD` with read-modify-write publishes and the identity
//! check, the epoch, extents with rotation and spares, checkpoints with the two-slot barrier, retirement and deletion,
//! ref moves with parking, idempotency, minimal markers and leases, `file mv` and `file rm` intents over `Vfs` renames,
//! and recovery — with the seeded-bug switches of [F16 §17] in their own module ([`bugs`]). It is the harness-validation
//! vehicle of E4 (every seeded bug found by the crash enumerator of WP-32) and the vehicle of measurements 1, 2, 12 and
//! T2 on the real Windows `Vfs` (PLAN §6.2 R10).
//!
//! Test-only crate; checked by GT20 (e) on every target. Filled by WP-40 and WP-40b (R-TOY). Sources:
//! `docs/spec/format/16-protocol.md` [F16] (every P-rule, §17's catalogue), [F04] (`HEAD`), [F05] (the log), [F03]
//! (`LOCK`, slots, anchors), [F13 §3.8] (I-G1–I-G6), [F15] (the fault model), [F17] (the parameters), [OS/lock],
//! [OS/proc §5, §6], [OS/clock §4]; [60 §3.1] item 4, [80 §2.4.4]; `docs/m0/PLAN.md` §2.2, §3.2 WP-40, §6.2 R10.
//!
//! Module map:
//!
//! | Module | Contents | Specification |
//! |---|---|---|
//! | [`bugs`] | [`Bug`], [`Bugs`], the catalogue [`CATALOGUE`], the open E4 items [`OPEN`] | [F16 §17] |
//! | [`config`] | [`Config`], the test and production profiles, [`commit_size`] | [F17 §3, §12], [60 §5.2] item 2 |
//! | [`codec`] | little-endian and LEB128 codecs, XXH3 | [F01 §5.2, §7.1] |
//! | [`format`] | `RecHdr`, groups and the chain, record validity, payloads, the extent head | [F05 §3–§9] |
//! | [`head`] | `HeadSlot`, slot validity and choice | [F04] |
//! | [`state`] | the replayed tables, all-or-nothing application, the scratch layer's undo log, the raw facts, the `HEAD` fold, segment snapshots | [F05 §10], [F16] P-30 |
//! | [`store`] | [`Toy`]: opening, `HEAD` reads, the chain-rule scan, the reader's view | [F16 §7, §8] |
//! | [`ops`] | [`Op`] and its records; decisions, allocation, the HLC | [F16 §5.1–§5.2, §5.6] |
//! | [`write`] | phases 2a and 2b, phase 3 ([`Toy::maintain`]), the publish, the durable publish, flags and `config_gen` | [F16 §5, §9] |
//! | [`recover`] | boot-change recovery, adoption, the recovering writer | [F16 §10] |
//! | [`maint`] | checkpoints, the barrier, deletion, spares, quiet bytes, `repair` of a damaged segment | [F16 §11, §12], [F03 §3.1], [80 §2.5] rule 8 |
//! | [`repair`] | `repair` of a store with no valid `HEAD` slot, from the extent heads | [F16] P-85, [F15] OP-1 |
//! | [`intent`] | `file mv`, `file rm`, intent slots and anchors, intent recovery | [F16 §13.1], [40 §3.4, §3.5] |
//! | [`init`](mod@init) | [`init()`](fn@init) | [F16 §13.7] |
//! | [`lock`] | the lock client and the in-process record of the lock-layer bugs | [OS/lock], [F16 §17.4] |
//! | [`tap`] | protocol notes for the trace predicates | [F13 §1.4] |
//! | [`verify`](mod@verify) | [`verify()`](fn@verify): the model checks over the raw facts | [F13 §3], [F16 §17.2] |

#![forbid(unsafe_code)]

pub mod bugs;
pub mod codec;
pub mod config;
pub mod format;
pub mod head;
pub mod init;
pub mod intent;
pub mod lock;
pub mod maint;
pub mod ops;
pub mod recover;
pub mod repair;
pub mod state;
pub mod store;
pub mod tap;
pub mod verify;
pub mod write;

pub use bugs::{Bug, BugInfo, Bugs, CATALOGUE, N_BUGS, OPEN};
pub use config::{Config, commit_size};
pub use init::init;
pub use intent::{DONE_TAG, FileOp, Live, file_name};
pub use lock::ProcLocks;
pub use maint::Checkpointed;
pub use ops::{ClaimOp, CommitOp, ForkOp, ORPHANS_BASE, Op, PROBE_REF, ReleaseOp, RuntimeOp};
pub use state::{MAIN, State};
pub use store::{Toy, ToyError, View};
pub use tap::{NoTap, Note, PublishNote, Step, Tap};
pub use verify::verify;
pub use write::Done;

/// Helpers shared by the unit tests of every module.
#[cfg(test)]
pub(crate) mod testing {
    /// The proptest configuration: 256 cases in the `pr` tier (the default), more in `nightly` and `exit`
    /// (`MOIRAI_TEST_TIER`, PLAN §2.1); no failure persistence.
    pub(crate) fn proptest_config() -> proptest::test_runner::Config {
        let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
            Ok("nightly") => 4_096,
            Ok("exit") => 65_536,
            _ => 256,
        };
        proptest::test_runner::Config {
            cases,
            failure_persistence: None,
            ..proptest::test_runner::Config::default()
        }
    }

    use std::path::Path;

    use moirai_vfs_sim::{SimConfig, SimVfs, SimWorld};

    use crate::{Config, NoTap, ProcLocks, Toy};

    /// The store directory of the unit tests' simulated worlds.
    pub(crate) const STORE: &str = "/sim/store";

    /// The epoch of [`sim_store`]'s image.
    pub(crate) const EPOCH: u64 = 0x5EED_0000_0000_0001;

    /// A store as `init` leaves it (its image, [`crate::init::image`]) in a fresh simulated world with `seed`: the world,
    /// a process's `Vfs`, and the image.
    pub(crate) fn sim_store(cfg: &Config, seed: u64) -> (SimWorld, SimVfs, crate::init::Image) {
        let w = SimWorld::new(SimConfig::new(seed));
        let boot = w.boot().1.0;
        let img = crate::init::image(cfg, EPOCH, [7; 16], boot, 1_790_000_000_000);
        let s = Path::new(STORE);
        w.mkdir_all(&s.join("tmp"));
        let put = |name: &str, b: &[u8]| {
            w.put_file(&s.join(name), b)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        };
        put("LOCK", &img.lock);
        let mut log = vec![0u8; cfg.extent_bytes as usize];
        log[..img.log.len()].copy_from_slice(&img.log);
        put("log.1", &log);
        put("HEAD", &img.head);
        let v = w.process_with("unit", None, Some(true));
        (w, v, img)
    }

    /// A handle on [`sim_store`]'s store.
    pub(crate) fn open(v: &SimVfs, cfg: &Config) -> Toy<SimVfs> {
        Toy::open(
            v.clone(),
            Path::new(STORE),
            cfg.clone(),
            NoTap,
            ProcLocks::new(),
        )
        .unwrap_or_else(|e| panic!("open: {e}"))
    }
}
