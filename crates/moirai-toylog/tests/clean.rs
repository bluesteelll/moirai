//! E4, first half (PLAN §3.2 WP-40): with every seeded-bug switch off, the crash enumerator of WP-32 finds no violation
//! in any scenario of the toy log — every acknowledged durable effect survives every crash state and every process
//! death, every operation is all or nothing, first reads are fresh, the model checks of `doctor --verify` hold, and the
//! trace predicates of [F13 §1.4] (I-G4, I-G6) and the simulator's protocol-violation checks ([F15 §3.13]) stay silent.
//!
//! Each test enumerates one scenario (`tests/common`) with every dimension family on, in the tier `MOIRAI_TEST_TIER`
//! names (PLAN §2.1): `pr` (the default: per-file prefixes plus one torn sector, the enumerator's 600 s budget) or
//! `nightly` and `exit` (every subset, every torn mix, random states, kills at every point; GT1's 10⁵ crash states per
//! enumeration). The nightly tiers add seeds. Each test prints its report, whose `Display` carries the state counts.

mod common;

use moirai_toylog::Bugs;
use moirai_vfs_sim::enumerate::{EnumConfig, Tier, enumerate};

/// The world seeds of a clean enumeration in `tier`.
fn seeds(tier: Tier) -> Vec<u64> {
    match tier {
        Tier::Pr => vec![1],
        Tier::Nightly => vec![1, 2, 3, 4],
        Tier::Exit => (1..=16).collect(),
    }
}

/// Enumerates `sc` with every switch off and asserts that nothing failed and that the enumeration completed.
fn clean(sc: common::Scenario) {
    let tier = Tier::from_env();
    let name = sc.name;
    let sub = common::ToySubject::new(sc, Bugs::NONE);
    let r = enumerate(&sub, &EnumConfig::new(tier, seeds(tier)));
    println!("{name}: {r}");
    r.assert_passed();
}

macro_rules! clean_tests {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                clean(common::$name());
            }
        )*

        /// The suite's tests are exactly the harness's scenarios: one test per scenario of `common::all()`, each named
        /// after the function that builds it and enumerating that scenario, with unique scenario names.
        #[test]
        fn every_scenario_has_its_test() {
            let tested: Vec<&str> = vec![$(common::$name().name),*];
            let all: Vec<&str> = common::all().iter().map(|s| s.name).collect();
            assert_eq!(tested, all, "the clean_tests! list and common::all() differ");
            let mut sorted = all.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), all.len(), "scenario names are unique: {all:?}");
        }
    };
}

clean_tests!(
    basic,
    leases,
    rotate,
    spare,
    retire,
    ckpt,
    gc,
    fork,
    intents,
    trash,
    xvol,
    boot,
    reboot,
    init_store,
    inproc,
    admin,
    import,
    server,
    timeouts,
    retry,
    intents_live,
    clock,
    quiet,
    sizes,
    auto,
    maint2,
    reads,
    overlay,
    lazytail,
    lost,
    bump,
    boundary,
    reuse,
    stale,
    barrier,
    fatal,
    misplaced,
    foreign,
    flags,
);
