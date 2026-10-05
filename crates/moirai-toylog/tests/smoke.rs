//! The toy log on the in-memory `Vfs`, without the enumerator: every scenario runs to its end, a reader sees what it
//! wrote, and the recovery finds nothing wrong; the slot decoder the enumerator's trace predicates read; and the
//! kept-view check of the toy's own harness ([`common::checks::KeptWatch`]).

mod common;

use std::path::{Path, PathBuf};

use common::checks::{KeptWatch, lowest_poisoned};
use moirai_toylog::head::SLOT_LEN;
use moirai_toylog::{Bugs, Replay, State, View};
use moirai_vfs_sim::enumerate::{Ledger, SlotDecode, Subject};
use moirai_vfs_sim::{SECTOR, SimConfig, SimWorld, Site};

#[test]
fn every_scenario_runs_and_recovers_cleanly() {
    for sc in common::all() {
        let name = sc.name;
        let sub = common::ToySubject::new(sc, Bugs::NONE);
        let w = SimWorld::new(sub.config(1));
        let l = Ledger::new(&w);
        sub.setup(&w, &l);
        sub.workload(&w, &l);
        let rec = sub.recover(&w);
        assert!(
            rec.first_read.is_ok() && rec.state.is_ok(),
            "{name}: {rec:?}"
        );
        assert!(rec.findings.is_empty(), "{name}: {:?}", rec.findings);
        assert!(rec.answered.is_empty(), "{name}: {:?}", rec.answered);
        let state = rec.state.unwrap();
        assert!(!state.is_empty(), "{name}: the recovered state is empty");
        println!("{name}: {} effects", state.len());
    }
}

/// [`common::decode_slot`] ([F04 §7]): `init`'s `HEAD` holds two valid slots, `slot_seq` 1 and 2, with the creator's
/// boot identity; the fixture of a fatal slot decodes as fatal, and a slot with a flipped bit as absent.
#[test]
fn the_slot_decoder_classifies_slots() {
    let sub = common::ToySubject::new(common::basic(), Bugs::NONE);
    let img = moirai_toylog::init::image(
        &sub.cfg(),
        common::EPOCH,
        common::STORE_ID,
        [9; 16],
        common::INIT_WALL_MS,
    );
    let slot = |b: &[u8], k: usize| common::decode_slot(&b[k * SLOT_LEN..(k + 1) * SLOT_LEN]);
    for k in 0..2 {
        let SlotDecode::Valid(v) = slot(&img.head, k) else {
            panic!("slot {k} of init's HEAD is not valid");
        };
        assert_eq!(v.slot_seq, k as u64 + 1);
        assert_eq!(v.boot_id, [9; 16]);
        assert_eq!(v.committed_lsn, v.durable_lsn);
        assert_eq!(v.monotone.len(), 15);
    }
    let fatal = common::fatal_newest_slot(&img.head);
    assert_eq!(slot(&fatal, 1), SlotDecode::Fatal);
    assert!(matches!(slot(&fatal, 0), SlotDecode::Valid(_)));
    let mut torn = img.head.clone();
    torn[100] ^= 1;
    assert_eq!(slot(&torn, 0), SlotDecode::Absent);
}

/// E of the kept-view tests: four sectors.
const KEPT_E: u64 = 4 * SECTOR;

/// A world whose store holds `extents` log extents of [`KEPT_E`] bytes, durable (no sector dirty or poisoned).
fn kept_world(extents: u32) -> SimWorld {
    let w = SimWorld::new(SimConfig::new(5));
    w.mkdir_all(Path::new(common::STORE));
    for n in 1..=extents {
        w.put_file(&log(n), &vec![0u8; KEPT_E as usize])
            .unwrap_or_else(|e| panic!("log.{n}: {e:?}"));
    }
    w
}

fn log(n: u32) -> PathBuf {
    Path::new(common::STORE).join(format!("log.{n}"))
}

/// A view with an empty state, bound `l0`, read under slot `slot_seq`, built from the set whose bound is `base`.
fn view(base: u64, l0: u64, slot_seq: u64) -> View {
    View {
        state: State::default(),
        l0,
        chain: 0,
        slot_seq,
        base,
    }
}

/// Makes the sector at `offset` of `log.<n>` poisoned (FM-3.1): an external write to it, then a flush that fails.
fn poison(w: &SimWorld, n: u32, offset: u64) {
    w.external_write(&log(n), offset, &[0xA5; 16])
        .unwrap_or_else(|e| panic!("{e:?}"));
    w.queue_choice(Site::FlushFault, 1);
    w.external_flush(&log(n))
        .unwrap_or_else(|e| panic!("{e:?}"));
}

/// A replay in which a valid group spans `l0`.
fn span(l0: u64) -> Replay {
    Replay::Spanned {
        start: l0 - 50,
        end: l0 + 50,
    }
}

/// [`KeptWatch`] ([F16] P-56): after a refresh that read a new slot, a replay that finds a valid group spanning the
/// view's bound, or that ends below it with an invalid group, is reported as a bound the valid log no longer has; a
/// replay that reaches the bound with another state is reported row by row; a replay that is unreached, or that reaches
/// the bound with the view's own state, reports nothing.
#[test]
fn the_kept_view_check_reports_a_bound_the_valid_log_lacks() {
    let w = kept_world(2);
    let watch = KeptWatch::begin(&w, &view(0, 100, 1), KEPT_E);
    let v = view(0, KEPT_E + 200, 2);
    let got = watch.judge(&w, &v, &span(KEPT_E + 200), "a reader's view");
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].starts_with(&format!(
            "kept view (P-56): a reader's view has the bound {}, which is no group boundary",
            KEPT_E + 200
        )) && got[0].contains(&format!("[{}, {})", KEPT_E + 150, KEPT_E + 250)),
        "{got:?}"
    );
    let got = watch.judge(&w, &v, &Replay::Short { end: 300 }, "a reader's view");
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].contains("beyond the end of the valid log")
            && got[0].contains("invalid group at 300"),
        "{got:?}"
    );
    let same = Replay::Reached(Box::default());
    assert!(watch.judge(&w, &v, &same, "a reader's view").is_empty());
    let mut other = State::default();
    other.runtime.insert(7, 70);
    let got = watch.judge(&w, &v, &Replay::Reached(Box::new(other)), "a reader's view");
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains("lacks lazy row 0x7 = 0x46"), "{got:?}");
    assert!(
        watch
            .judge(&w, &v, &Replay::Unreached, "a reader's view")
            .is_empty()
    );
}

/// [`KeptWatch`] after a refresh that read no new slot (P-56 re-checks a bound only under a new `slot_seq`; under the
/// same slot a view a lost lazy tail made stale is kept, [F16] P-58): nothing is judged.
#[test]
fn the_kept_view_check_judges_nothing_under_the_same_slot() {
    let w = kept_world(1);
    let watch = KeptWatch::begin(&w, &view(0, 100, 4), KEPT_E);
    let v = view(0, 200, 4);
    assert!(watch.judge(&w, &v, &span(200), "v").is_empty());
    assert!(
        watch
            .judge(&w, &v, &Replay::Short { end: 120 }, "v")
            .is_empty()
    );
    assert_eq!(watch.judge(&w, &view(0, 200, 5), &span(200), "v").len(), 1);
}

/// [`KeptWatch`] after a failed flush since the watch began (FM-3.6: the flush may take back a lazy tail, and the next
/// flush re-writes the pending range): a span is not judged, wherever the flush was.
#[test]
fn the_kept_view_check_judges_no_span_after_a_failed_flush() {
    let w = kept_world(2);
    let watch = KeptWatch::begin(&w, &view(0, 100, 1), KEPT_E);
    let v = view(0, 200, 2);
    assert_eq!(watch.judge(&w, &v, &span(200), "a writer's view").len(), 1);
    // A failed flush beyond the bound, in another extent.
    poison(&w, 2, 3 * SECTOR);
    assert_eq!(w.failed_flushes(), 1);
    assert!(
        watch
            .judge(&w, &v, &span(200), "a writer's view")
            .is_empty()
    );
    // A watch that begins after it judges again: the poisoned sector lies beyond the bound.
    let later = KeptWatch::begin(&w, &view(0, 100, 1), KEPT_E);
    assert_eq!(later.judge(&w, &v, &span(200), "a writer's view").len(), 1);
}

/// [`KeptWatch`] with a sector poisoned before the watch began (FM-3.2: reads of it may differ): nothing is judged when
/// the sector lies below the view's bound, from the extent of the view's base on, at either question — even after a
/// re-write ended the poison between them; a poisoned sector at or beyond the bound, or in an extent below the base's,
/// leaves the check in force.
#[test]
fn the_kept_view_check_judges_nothing_over_a_poisoned_sector() {
    let w = kept_world(3);
    poison(&w, 2, SECTOR + 7);
    let p = KEPT_E + SECTOR;
    let judge = |watch: &KeptWatch, l0: u64| watch.judge(&w, &view(0, l0, 2), &span(l0), "v");
    let watch = KeptWatch::begin(&w, &view(0, 100, 1), KEPT_E);
    assert!(judge(&watch, p + 1).is_empty());
    assert!(judge(&watch, 2 * KEPT_E + 60).is_empty());
    // A read of the bytes below the sector's first byte never reaches it.
    assert_eq!(judge(&watch, p).len(), 1);
    // A view whose base lies in the next extent reads nothing of log.2.
    let above = KeptWatch::begin(&w, &view(2 * KEPT_E, 2 * KEPT_E + 10, 1), KEPT_E);
    let l0 = 2 * KEPT_E + 60;
    let got = above.judge(&w, &view(2 * KEPT_E, l0, 2), &span(l0), "v");
    assert_eq!(got.len(), 1);
    // A write ends the poison (FM-3.5); the watch that began before it still judges nothing below the sector.
    w.external_write(&log(2), SECTOR, &[1; 4])
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(lowest_poisoned(&w, 0, KEPT_E), u64::MAX);
    assert!(judge(&watch, p + 1).is_empty());
    let after = KeptWatch::begin(&w, &view(0, 100, 1), KEPT_E);
    assert_eq!(judge(&after, p + 1).len(), 1);
    // A sector poisoned after the watch began comes with a failed flush, which the watch sees too.
    poison(&w, 1, 0);
    assert!(judge(&after, 60).is_empty());
}

/// [`lowest_poisoned`]: the first byte of the lowest poisoned sector of the log from the extent of `from` on, by
/// extent and sector, up to the first extent that does not exist; a poisoned sector below `from` in its extent counts.
#[test]
fn lowest_poisoned_finds_the_first_poisoned_sector() {
    let w = kept_world(3);
    assert_eq!(lowest_poisoned(&w, 0, KEPT_E), u64::MAX);
    poison(&w, 3, 3 * SECTOR);
    assert_eq!(lowest_poisoned(&w, 0, KEPT_E), 2 * KEPT_E + 3 * SECTOR);
    poison(&w, 2, 2 * SECTOR + 1);
    poison(&w, 2, 3 * SECTOR);
    assert_eq!(lowest_poisoned(&w, 0, KEPT_E), KEPT_E + 2 * SECTOR);
    assert_eq!(
        lowest_poisoned(&w, KEPT_E + 3 * SECTOR, KEPT_E),
        KEPT_E + 2 * SECTOR
    );
    assert_eq!(
        lowest_poisoned(&w, 2 * KEPT_E, KEPT_E),
        2 * KEPT_E + 3 * SECTOR
    );
    // The extents stop at the first that does not exist.
    assert_eq!(lowest_poisoned(&w, 3 * KEPT_E, KEPT_E), u64::MAX);
    poison(&w, 1, 0);
    assert_eq!(lowest_poisoned(&w, 0, KEPT_E), 0);
}
