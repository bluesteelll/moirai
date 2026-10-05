//! What every differential test shares: the test tier and the property-test runner of each tier
//! (`docs/m0/PLAN.md` §2.1).

use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

/// The test tier: `pr` (default), `nightly` or `exit` (`MOIRAI_TEST_TIER`, PLAN §2.1).
pub fn tier() -> &'static str {
    match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => "nightly",
        Ok("exit") => "exit",
        _ => "pr",
    }
}

/// A property-test runner of `base` cases at tier `pr`, 16 times as many at `nightly` and 64 at `exit`, from a fixed
/// seed per tier and test name, with nothing persisted: a failure is reproduced by the same run.
pub fn runner(name: &str, base: u32) -> TestRunner {
    let (scale, tag) = match tier() {
        "nightly" => (16, 2u8),
        "exit" => (64, 3u8),
        _ => (1, 1u8),
    };
    let mut seed = [tag; 32];
    for (i, b) in name.bytes().enumerate() {
        seed[i % 32] ^= b.rotate_left(i as u32 % 8);
    }
    TestRunner::new_with_rng(
        Config {
            cases: base * scale,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &seed),
    )
}
