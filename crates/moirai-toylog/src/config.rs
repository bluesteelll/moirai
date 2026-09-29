//! The toy's parameters: the store parameters of [F17] it reads, the injected in-lock cost of measurement 2, the
//! commit-size distribution, and the seeded-bug switches.

use crate::bugs::Bugs;
use crate::format::InitParams;

/// The toy's configuration. [`Config::test_profile`] is [F17 §12]'s test profile; [`Config::production`] the production
/// values of [F17 §3] (the holes at their design figures).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// P01 `store.log-extent-bytes`, E: a power of two in [2^16, 2^30] (init-fixed).
    pub extent_bytes: u64,
    /// P03 `store.hist-frame-commits` (init-fixed; carried in `InitParams`).
    pub hist_frame_commits: u32,
    /// P04 `store.hist-frame-bytes` (init-fixed).
    pub hist_frame_bytes: u32,
    /// P02 `store.log-active-extents`.
    pub active_extents: u32,
    /// P05 `store.commit.inline-max-bytes` (W1).
    pub inline_max_bytes: u32,
    /// P26 `lock.writer-wait-ms`.
    pub writer_wait_ms: u32,
    /// P27 `lock.flush-wait-ms`.
    pub flush_wait_ms: u32,
    /// P34 `gc.delete-grace`, in ms.
    pub delete_grace_ms: u64,
    /// `lease.ttl-default`, in ms.
    pub lease_ttl_ms: u64,
    /// Phase 3's checkpoint trigger ([F16] P-51, [F17 §5]): [`crate::Toy::maintain`], which a caller runs after it has
    /// reported an operation's acknowledgement, runs maintenance when the tail `committed_lsn − checkpoint_lsn` reaches this
    /// many bytes; 0 = never (maintenance runs only when asked).
    pub auto_checkpoint_bytes: u64,
    /// Measurement 2's injected in-lock cost, in ns (PLAN §6.2 R10 as amended by WP-80a): once per append, in the
    /// holding of the writer byte that appends (`WriterDiag` activity 1, [F03 §6.1]), right before the append's first
    /// `write_at`, the appender waits this long on its monotonic clock. No other holding of the writer byte (the flush
    /// holder's scan and publish, a maintenance or barrier publish, a `HEAD`-only publish) carries it, and a restart of
    /// phase 2a at P-28 carries it only when it appends. The wait polls a `Vfs` call that is a scheduling point of the
    /// in-memory `Vfs` (whose clock advances only there), so it ends on the simulator too.
    pub in_lock_cost_ns: u64,
    /// Keep the raw facts of every applied record ([`crate::state::Fact`]) for [`crate::verify()`]: a harness turns it
    /// on; off (the profiles' default, and every measurement), nothing in the state grows with the log's history.
    pub facts: bool,
    /// The seeded bugs switched on.
    pub bugs: Bugs,
}

impl Config {
    /// [F17 §12]'s test profile: E = 64 KiB, two active extents, 4 KiB inline commits, the production lock waits
    /// ("as production", design figure 2,000 ms), a 1 s deletion grace.
    pub fn test_profile() -> Config {
        Config {
            extent_bytes: 1 << 16,
            hist_frame_commits: 4,
            hist_frame_bytes: 4096,
            active_extents: 2,
            inline_max_bytes: 4096,
            writer_wait_ms: 2_000,
            flush_wait_ms: 2_000,
            delete_grace_ms: 1_000,
            lease_ttl_ms: 15 * 60 * 1000,
            auto_checkpoint_bytes: 0,
            in_lock_cost_ns: 0,
            facts: false,
            bugs: Bugs::NONE,
        }
    }

    /// [F17 §3]'s production values: E = 64 MiB, four active extents, 1 MiB inline commits, 2 s lock waits, a 60 s
    /// deletion grace, a 15-minute lease TTL.
    pub fn production() -> Config {
        Config {
            extent_bytes: 64 << 20,
            hist_frame_commits: 256,
            hist_frame_bytes: 1 << 20,
            active_extents: 4,
            inline_max_bytes: 1 << 20,
            writer_wait_ms: 2_000,
            flush_wait_ms: 2_000,
            delete_grace_ms: 60_000,
            lease_ttl_ms: 15 * 60 * 1000,
            auto_checkpoint_bytes: 0,
            in_lock_cost_ns: 0,
            facts: false,
            bugs: Bugs::NONE,
        }
    }

    /// The same configuration with `bugs` switched on.
    pub fn with_bugs(mut self, bugs: Bugs) -> Config {
        self.bugs = bugs;
        self
    }

    /// The init-fixed block `init` writes, with `store_id`.
    pub fn init_params(&self, store_id: [u8; 16]) -> InitParams {
        InitParams {
            log_extent_bytes: self.extent_bytes,
            hist_frame_commits: self.hist_frame_commits,
            hist_frame_bytes: self.hist_frame_bytes,
            store_id,
        }
    }
}

/// The spec-derived commit-size distribution of measurement 2 ([60 §5.2] item 2): typical commits of 0.3–0.6 KB and a
/// tail up to `store.commit.inline-max-bytes`. `draw` is a uniform random `u64` (the caller's randomness); the result is
/// the commit's filler size in bytes: nine in ten commits uniform in [300, 600], one in ten log-uniform in
/// [600, `inline_max`].
pub fn commit_size(draw: u64, inline_max: u32) -> u32 {
    let tail = draw.is_multiple_of(10);
    let r = draw / 10;
    if !tail || inline_max <= 600 {
        return 300 + (r % 301) as u32;
    }
    // Log-uniform: 600 · (inline_max / 600)^u with u in [0, 1), in 1/1024 steps.
    let u = (r % 1024) as f64 / 1024.0;
    let ratio = f64::from(inline_max) / 600.0;
    let size = 600.0 * ratio.powf(u);
    (size as u32).clamp(600, inline_max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_follow_f17() {
        let t = Config::test_profile();
        assert_eq!(t.extent_bytes, 65_536);
        assert!(t.init_params([1; 16]).valid());
        assert!(Config::production().init_params([1; 16]).valid());
        // C-1: P05 ≤ P01 / 8.
        assert!(u64::from(t.inline_max_bytes) <= t.extent_bytes / 8);
    }

    #[test]
    fn commit_sizes_stay_in_range() {
        let mut typical = 0;
        for d in 0..10_000u64 {
            let s = commit_size(d.wrapping_mul(0x9E37_79B9_7F4A_7C15), 4096);
            assert!((300..=4096).contains(&s));
            if s <= 600 {
                typical += 1;
            }
        }
        assert!(typical > 8_000, "{typical}");
    }
}
